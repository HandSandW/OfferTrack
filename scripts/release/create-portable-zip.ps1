param(
    [Parameter(Mandatory = $true)]
    [string]$Source,
    [Parameter(Mandatory = $true)]
    [string]$Destination
)

$ErrorActionPreference = 'Stop'
$sourcePath = [System.IO.Path]::GetFullPath($Source)
$destinationPath = [System.IO.Path]::GetFullPath($Destination)

if (-not (Test-Path -LiteralPath $sourcePath -PathType Container)) {
    throw 'Portable source directory does not exist.'
}

$entries = @(Get-ChildItem -LiteralPath $sourcePath -Force -Recurse)
$files = @($entries | Where-Object { -not $_.PSIsContainer })
if ($files.Count -eq 0) {
    throw 'Portable source must contain at least one regular file.'
}
if ($entries | Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint }) {
    throw 'Portable source must not contain links or junctions.'
}
if ($files | Where-Object { -not ($_.Attributes -band [System.IO.FileAttributes]::Archive) -and -not ($_.Attributes -band [System.IO.FileAttributes]::Normal) }) {
    throw 'Portable source contains a special file.'
}

Add-Type -AssemblyName System.IO.Compression
$stream = [System.IO.File]::Open($destinationPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
    $archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Create, $true)
    try {
        $rootName = [System.IO.Path]::GetFileName($sourcePath.TrimEnd([System.IO.Path]::DirectorySeparatorChar))
        foreach ($item in ($files | Sort-Object FullName)) {
            $relativeName = $item.FullName.Substring($sourcePath.Length).TrimStart([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar).Replace('\', '/')
            $entry = $archive.CreateEntry("$rootName/$relativeName", [System.IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = [System.DateTimeOffset]::new(1980, 1, 1, 0, 0, 0, [System.TimeSpan]::Zero)
            $input = [System.IO.File]::Open($item.FullName, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
            try {
                $output = $entry.Open()
                try {
                    $input.CopyTo($output)
                }
                finally {
                    $output.Dispose()
                }
            }
            finally {
                $input.Dispose()
            }
        }
    }
    finally {
        $archive.Dispose()
    }
    $stream.Flush($true)
}
finally {
    $stream.Dispose()
}
