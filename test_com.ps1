$clsid = "{A5C7B3F1-2E4D-4A8B-9C1F-3D7E6F8A9B2C}"
try {
    $obj = [System.Activator]::CreateInstance([System.Type]::GetTypeFromCLSID($clsid))
    Write-Host "COM object created successfully: $obj"
} catch {
    Write-Host "Failed to create COM object: $_"
}
