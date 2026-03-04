#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::UI::Shell::*;

const CLSID_AICODE: GUID = GUID {
    data1: 0xA5C7B3F1,
    data2: 0x2E4D,
    data3: 0x4A8B,
    data4: [0x9C, 0x1F, 0x3D, 0x7E, 0x6F, 0x8A, 0x9B, 0x2C],
};

static DLL_MODULE: AtomicIsize = AtomicIsize::new(0);
static OBJ_COUNT: AtomicUsize = AtomicUsize::new(0);

// ── DLL entry point ──────────────────────────────────────────────────────────

#[no_mangle]
extern "system" fn DllMain(
    hinst: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    if reason == 1 {
        // DLL_PROCESS_ATTACH
        DLL_MODULE.store(hinst as isize, Ordering::Relaxed);
    }
    1 // TRUE
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn dll_dir() -> Result<String> {
    let hmod = HMODULE(DLL_MODULE.load(Ordering::Relaxed) as _);
    let mut buf = [0u16; 260];
    let len = unsafe { GetModuleFileNameW(hmod, &mut buf) } as usize;
    if len == 0 {
        return Err(Error::from(E_FAIL));
    }
    let path = String::from_utf16_lossy(&buf[..len]);
    path.rfind('\\')
        .map(|pos| path[..=pos].to_string())
        .ok_or_else(|| Error::from(E_FAIL))
}

fn exe_path() -> Result<String> {
    Ok(format!("{}aicode-bat-gui.exe", dll_dir()?))
}

unsafe fn alloc_com_str(s: &str) -> Result<PWSTR> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let size = wide.len() * std::mem::size_of::<u16>();
    let ptr = CoTaskMemAlloc(size) as *mut u16;
    if ptr.is_null() {
        return Err(Error::from(E_OUTOFMEMORY));
    }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
    Ok(PWSTR(ptr))
}

// ── DLL exports ──────────────────────────────────────────────────────────────

#[no_mangle]
extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJ_COUNT.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();

    if *rclsid != CLSID_AICODE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    let factory: IClassFactory = AiCodeCommandFactory.into();
    if *riid == IClassFactory::IID || *riid == IUnknown::IID {
        *ppv = factory.as_raw();
        std::mem::forget(factory);
        S_OK
    } else {
        E_NOINTERFACE
    }
}

// ── IClassFactory ────────────────────────────────────────────────────────────

#[implement(IClassFactory)]
struct AiCodeCommandFactory;

impl IClassFactory_Impl for AiCodeCommandFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        if punkouter.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }
        unsafe {
            *ppvobject = std::ptr::null_mut();
            let cmd: IExplorerCommand = AiCodeCommand.into();
            if *riid == IExplorerCommand::IID || *riid == IUnknown::IID {
                *ppvobject = cmd.as_raw();
                std::mem::forget(cmd);
                OBJ_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(())
            } else {
                Err(Error::from(E_NOINTERFACE))
            }
        }
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.as_bool() {
            OBJ_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            OBJ_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

// ── IExplorerCommand ─────────────────────────────────────────────────────────

#[implement(IExplorerCommand)]
struct AiCodeCommand;

impl IExplorerCommand_Impl for AiCodeCommand_Impl {
    fn GetTitle(&self, _psiitemarray: Option<&IShellItemArray>) -> Result<PWSTR> {
        unsafe { alloc_com_str("AICode 启动器") }
    }

    fn GetIcon(&self, _psiitemarray: Option<&IShellItemArray>) -> Result<PWSTR> {
        unsafe { alloc_com_str(&exe_path()?) }
    }

    fn GetToolTip(&self, _psiitemarray: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(Error::from(E_NOTIMPL))
    }

    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(CLSID_AICODE)
    }

    fn GetState(
        &self,
        _psiitemarray: Option<&IShellItemArray>,
        _fokaliasedrop: BOOL,
    ) -> Result<u32> {
        Ok(0) // ECS_ENABLED
    }

    fn Invoke(
        &self,
        psiitemarray: Option<&IShellItemArray>,
        _pbc: Option<&IBindCtx>,
    ) -> Result<()> {
        let exe = exe_path()?;
        let mut dir = String::new();

        if let Some(items) = psiitemarray {
            unsafe {
                if let Ok(item) = items.GetItemAt(0) {
                    if let Ok(path) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                        dir = path.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(path.0 as *const c_void));
                    }
                }
            }
        }

        let mut cmd = std::process::Command::new(&exe);
        if !dir.is_empty() {
            cmd.arg(&dir);
        }
        let _ = cmd.spawn();
        Ok(())
    }

    fn GetFlags(&self) -> Result<u32> {
        Ok(0) // ECF_DEFAULT
    }

    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        Err(Error::from(E_NOTIMPL))
    }
}
