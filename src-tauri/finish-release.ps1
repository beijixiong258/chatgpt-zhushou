$ErrorActionPreference = 'Stop'
$metadata = cargo metadata --no-deps --format-version 1 --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw '无法定位构建目录' }
$release = Join-Path (($metadata | ConvertFrom-Json).target_directory) 'release/chatgpt对话助手.exe'
if (!(Test-Path -LiteralPath $release -PathType Leaf)) { throw "找不到发布程序 $release" }

# Embed the project notice in the standalone EXE and omit Tauri's generated version.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class ReleaseResources {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern IntPtr BeginUpdateResourceW(string path, bool deleteExisting);
    [DllImport("kernel32", SetLastError=true)]
    static extern bool UpdateResourceW(IntPtr handle, IntPtr type, IntPtr name, ushort lang, IntPtr data, uint count);
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true, EntryPoint="UpdateResourceW", ExactSpelling=true)]
    static extern bool WriteResource(IntPtr handle, IntPtr type, string name, ushort lang, byte[] data, uint count);
    [DllImport("kernel32", SetLastError=true)]
    static extern bool EndUpdateResourceW(IntPtr handle, bool discard);
    public static void FinalizeFile(string path, byte[] license, bool removeVersion) {
        var handle = BeginUpdateResourceW(path, false);
        if (handle == IntPtr.Zero) throw new Win32Exception();
        try {
            // RT_RCDATA, neutral language. The bytes come directly from the project's LICENSE.
            if (!WriteResource(handle, (IntPtr)10, "LICENSE", 0, license, (uint)license.Length))
                throw new Win32Exception();
            if (removeVersion && !UpdateResourceW(handle, (IntPtr)16, (IntPtr)1, 0x0409, IntPtr.Zero, 0))
                throw new Win32Exception();
        } catch {
            EndUpdateResourceW(handle, true);
            throw;
        }
        if (!EndUpdateResourceW(handle, false)) throw new Win32Exception();
    }
}
'@
$license = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot '../LICENSE'))
if ($license.Length -eq 0) { throw '项目 LICENSE 为空' }
[ReleaseResources]::FinalizeFile($release, $license, [bool](Get-Item -LiteralPath $release).VersionInfo.FileVersion)
$info = (Get-Item -LiteralPath $release).VersionInfo
if ($info.FileVersion -or $info.ProductVersion) { throw '程序仍含有版本号资源' }
