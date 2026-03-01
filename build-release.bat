@echo off
cd /d %~dp0
cargo build --release
if %errorlevel% neq 0 (
    echo 编译失败！
    pause
    exit /b 1
)
copy /y target\release\aicode-bat-gui.exe exe\aicode-bat-gui.exe
echo.
echo ✅ 编译并复制完成：exe\aicode-bat-gui.exe
pause
