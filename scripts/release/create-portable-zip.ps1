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

$items = @(Get-ChildItem -LiteralPath $sourcePath -Force)
if ($items.Count -eq 0 -or ($items | Where-Object { -not $_.PSIsContainer -and ($_.Attributes -band [System.IO.FileAttributes]::ReparsePoint) }) -or ($items | Where-Object { $_.PSIsContainer })) {
    throw 'Portable source must contain only regular root files.'
}

Add-Type -AssemblyName System.IO.Compression
$stream = [System.IO.File]::Open($destinationPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
try {
    $archive = [System.IO.Compression.ZipArchive]::new($stream, [System.IO.Compression.ZipArchiveMode]::Create, $true)
    try {
        $rootName = [System.IO.Path]::GetFileName($sourcePath.TrimEnd([System.IO.Path]::DirectorySeparatorChar))
        foreach ($item in ($items | Sort-Object Name)) {
            $entry = $archive.CreateEntry("$rootName/$($item.Name)", [System.IO.Compression.CompressionLevel]::Optimal)
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
