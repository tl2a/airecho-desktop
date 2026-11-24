param(
    [Parameter(Mandatory = $true)]
    [string] $Msys2Root,

    [Parameter(Mandatory = $true)]
    [ValidateSet("ucrt64", "clangarm64")]
    [string] $Environment,

    [Parameter(Mandatory = $true)]
    [string] $Executable,

    [Parameter(Mandatory = $true)]
    [string] $OutputDirectory
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$msys2Root = (Resolve-Path $Msys2Root).Path
$prefix = Join-Path $msys2Root $Environment
$bin = Join-Path $prefix "bin"
$exeSource = (Resolve-Path (Join-Path $repoRoot $Executable)).Path
$bundle = [System.IO.Path]::GetFullPath($OutputDirectory)
$targetRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot "target")).TrimEnd("\") + "\"
if (-not $bundle.StartsWith($targetRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "OutputDirectory must be inside the repository target directory: $targetRoot"
}
$ldd = Join-Path $msys2Root "usr\bin\ldd.exe"
$pixbufQuery = Join-Path $bin "gdk-pixbuf-query-loaders.exe"
$schemaCompiler = Join-Path $bin "glib-compile-schemas.exe"

foreach ($required in @($ldd, $pixbufQuery, $schemaCompiler)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Required MSYS2 packaging tool is missing: $required"
    }
}

$env:PATH = "$bin;$(Join-Path $msys2Root 'usr\bin');$env:PATH"
$loaderSourceDir = Join-Path $prefix "lib\gdk-pixbuf-2.0\2.10.0\loaders"
$svgLoader = Get-ChildItem -LiteralPath $loaderSourceDir -Filter "*svg*.dll" |
    Select-Object -First 1
if (-not $svgLoader) {
    throw "MSYS2 gdk-pixbuf SVG loader was not found in $loaderSourceDir"
}

$dependencyNames = @{}
foreach ($binary in @($exeSource, $svgLoader.FullName)) {
    $dependencyOutput = @(& $ldd $binary 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "ldd failed for $binary`: $($dependencyOutput -join [Environment]::NewLine)"
    }

    foreach ($line in $dependencyOutput) {
        if ($line -match "(?i)=>\s+not found") {
            throw "Unresolved runtime dependency for $binary`: $line"
        }
        if ($line -match "(?i)=>\s+/(?:ucrt64|clangarm64)/bin/(?<name>[^ \t\r\n]+\.dll)") {
            $dependencyNames[$Matches["name"]] = $true
        }
    }
}
if ($dependencyNames.Count -eq 0) {
    throw "No MSYS2 runtime DLL dependencies were found for $exeSource"
}

if (Test-Path -LiteralPath $bundle) {
    Remove-Item -LiteralPath $bundle -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $bundle | Out-Null
Copy-Item -LiteralPath $exeSource -Destination (Join-Path $bundle "AirEcho.exe")

foreach ($name in $dependencyNames.Keys) {
    $source = Join-Path $bin $name
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "MSYS2 dependency DLL is missing: $source"
    }
    Copy-Item -LiteralPath $source -Destination $bundle
}

$share = Join-Path $bundle "share"
New-Item -ItemType Directory -Force -Path $share | Out-Null
foreach ($relative in @("gtk-4.0", "icons\Adwaita", "icons\AdwaitaLegacy", "fontconfig", "licenses")) {
    $source = Join-Path (Join-Path $prefix "share") $relative
    if (Test-Path -LiteralPath $source -PathType Container) {
        $destination = Join-Path $share $relative
        $destinationParent = Split-Path -Parent $destination
        New-Item -ItemType Directory -Force -Path $destinationParent | Out-Null
        Copy-Item -LiteralPath $source -Destination $destination -Recurse -Force
    }
}
foreach ($theme in @("Adwaita", "AdwaitaLegacy")) {
    $themeIndex = Join-Path $share "icons\$theme\index.theme"
    if (-not (Test-Path -LiteralPath $themeIndex -PathType Leaf)) {
        throw "Required GTK icon theme was not packaged: $themeIndex"
    }
}

$fontConfigSource = Join-Path $prefix "etc\fonts"
if (Test-Path -LiteralPath $fontConfigSource -PathType Container) {
    Copy-Item -LiteralPath $fontConfigSource -Destination (Join-Path $bundle "etc") -Recurse -Force
}

$schemaSource = Join-Path $prefix "share\glib-2.0\schemas"
$schemaDestination = Join-Path $share "glib-2.0\schemas"
New-Item -ItemType Directory -Force -Path $schemaDestination | Out-Null
Get-ChildItem -LiteralPath $schemaSource -Filter "org.gtk.gtk4.*.gschema.xml" |
    Copy-Item -Destination $schemaDestination
& $schemaCompiler $schemaDestination
if ($LASTEXITCODE -ne 0) {
    throw "Failed to compile bundled GTK GSettings schemas"
}

$loaderDestination = Join-Path $bundle "lib\gdk-pixbuf-2.0\2.10.0\loaders"
New-Item -ItemType Directory -Force -Path $loaderDestination | Out-Null
Copy-Item -LiteralPath $svgLoader.FullName -Destination $loaderDestination

$cacheLines = @(& $pixbufQuery $svgLoader.FullName)
if ($LASTEXITCODE -ne 0) {
    throw "gdk-pixbuf-query-loaders failed for $($svgLoader.FullName)"
}
$moduleLine = -1
for ($index = 0; $index -lt $cacheLines.Count; $index++) {
    if ($cacheLines[$index].Trim().Contains("pixbufloader_svg.dll")) {
        $moduleLine = $index
        break
    }
}
if ($moduleLine -lt 0) {
    throw "Could not locate the SVG module path in the generated loader cache"
}
$cacheLines[$moduleLine] = '"@LOADERS_DIR@\\pixbufloader_svg.dll"'
$cacheLines = @($cacheLines | ForEach-Object {
    $_ -replace "^# LoaderDir = .*$", "# LoaderDir = @LOADERS_DIR@"
})
$cachePath = Join-Path $bundle "lib\gdk-pixbuf-2.0\2.10.0\loaders.cache.in"
[System.IO.File]::WriteAllLines($cachePath, $cacheLines, [System.Text.UTF8Encoding]::new($false))

$safePath = ($env:PATH -split ";" | Where-Object {
    $_ -and -not $_.StartsWith($msys2Root, [StringComparison]::OrdinalIgnoreCase)
}) -join ";"
$env:PATH = "$bundle;$safePath"
$help = @(& (Join-Path $bundle "AirEcho.exe") --help 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "Bundled AirEcho failed its startup smoke test: $($help -join [Environment]::NewLine)"
}

Write-Output "Packaged Windows runtime in $bundle"
