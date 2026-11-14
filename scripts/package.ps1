param(
    [Parameter(Mandatory)]
    [ValidateSet('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'aarch64-apple-darwin')]
    [string]$Target,
    [string]$OutputDirectory = 'dist'
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
function Invoke-ScarpProcess {
    param([string]$Executable, [string[]]$Arguments, [byte[]]$InputBytes = @())
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    $stdout = [System.IO.MemoryStream]::new()
    $stderr = [System.IO.MemoryStream]::new()
    try {
        if (-not $process.Start()) { throw 'Could not start packaged executable' }
        $readOut = $process.StandardOutput.BaseStream.CopyToAsync($stdout)
        $readErr = $process.StandardError.BaseStream.CopyToAsync($stderr)
        if ($InputBytes.Length -gt 0) {
            $process.StandardInput.BaseStream.Write($InputBytes, 0, $InputBytes.Length)
        }
        $process.StandardInput.Close()
        $process.WaitForExit()
        [void]$readOut.GetAwaiter().GetResult()
        [void]$readErr.GetAwaiter().GetResult()
        return @{ ExitCode = $process.ExitCode; Stdout = $stdout.ToArray(); Stderr = $stderr.ToArray() }
    }
    finally { $process.Dispose(); $stdout.Dispose(); $stderr.Dispose() }
}

$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $projectRoot
try {
    $metadataText = & cargo metadata --locked --offline --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not read package metadata' }
    $metadata = $metadataText | ConvertFrom-Json
    $version = ($metadata.packages | Where-Object { $_.name -eq 'scarp' }).version
    if ($version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') {
        throw 'Expected a three-part package version'
    }
    $compiler = & rustc --version
    if ($LASTEXITCODE -ne 0) { throw 'Could not read compiler version' }
    $compilerDetails = & rustc --version --verbose
    if ($LASTEXITCODE -ne 0) { throw 'Could not read compiler target' }
    if ($compilerDetails -notcontains "host: $Target") {
        throw "Packaging requires a native $Target toolchain"
    }
    $revision = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not identify the source commit' }
    $changes = & git status --porcelain=v1
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the working tree' }
    $treeState = if ($changes) { 'modified' } else { 'clean' }
    $packageName = "scarp-$version-$Target"
    $isWindowsTarget = $Target -eq 'x86_64-pc-windows-msvc'
    $extension = if ($isWindowsTarget) { '.zip' } else { '.tar.gz' }
    $outputPath = if ([System.IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory } else { Join-Path $projectRoot $OutputDirectory }
    $outputRoot = [System.IO.Path]::GetFullPath($outputPath)
    $archive = Join-Path $outputRoot ($packageName + $extension)
    if ((Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath ($archive + '.sha256'))) {
        throw 'Package output already exists; choose a fresh output directory'
    }

    & cargo build --locked --offline --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    $stagingRoot = Join-Path $projectRoot ('target/packages/' + [guid]::NewGuid().ToString('N'))
    $packageRoot = Join-Path $stagingRoot $packageName
    New-Item -ItemType Directory -Path $packageRoot -Force | Out-Null
    New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
    $binaryName = if ($isWindowsTarget) { 'scarp.exe' } else { 'scarp' }
    $binary = Join-Path $projectRoot "target/$Target/release/$binaryName"
    Copy-Item -LiteralPath $binary -Destination $packageRoot
    foreach ($filename in @('LICENSE', 'README.md', 'CHANGELOG.md', 'CONTRIBUTING.md')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot $filename) -Destination $packageRoot
    }
    foreach ($directory in @('examples', 'docs')) {
        Copy-Item -LiteralPath (Join-Path $projectRoot $directory) -Destination $packageRoot -Recurse
    }
    if ($version -in @('0.0.5-alpha.2', '0.0.5-alpha.3', '0.0.5-alpha.4', '0.0.5-alpha.5', '0.0.6-alpha.1', '0.0.6-alpha.2', '0.0.6-alpha.3', '0.0.6-alpha.4', '0.0.6-alpha.5', '0.0.6-alpha.6', '0.0.6-alpha.7', '0.0.6-alpha.8', '0.0.6-alpha.9')) {
        $releaseMetadataPath = Join-Path $projectRoot '.github/release-metadata.json'
        $releaseMetadata = Get-Content -LiteralPath $releaseMetadataPath -Raw | ConvertFrom-Json
        if ($releaseMetadata.schema_version -cne 1 -or $releaseMetadata.version -cne $version -or $releaseMetadata.documentation_repository -cne 'https://github.com/scarp-lang/scarp-docs') {
            throw 'Release metadata must match the package version and official documentation repository'
        }
        Copy-Item -LiteralPath $releaseMetadataPath -Destination (Join-Path $packageRoot 'RELEASE-METADATA.json')
    }
    $buildInfo = "version=$version`ntarget=$Target`ncommit=$revision`nworking_tree=$treeState`ncompiler=$compiler`n"
    [System.IO.File]::WriteAllText((Join-Path $packageRoot 'BUILD-INFO.txt'), $buildInfo, [System.Text.UTF8Encoding]::new($false))

    if ($isWindowsTarget) {
        Compress-Archive -LiteralPath $packageRoot -DestinationPath $archive
    }
    else {
        & chmod +x (Join-Path $packageRoot $binaryName)
        if ($LASTEXITCODE -ne 0) { throw 'Could not set executable permissions' }
        & tar -czf $archive -C $stagingRoot $packageName
        if ($LASTEXITCODE -ne 0) { throw 'Archive creation failed' }
    }

    # Verify the extracted deliverable, including its executable permissions.
    $extractedRoot = Join-Path $stagingRoot 'extracted'
    New-Item -ItemType Directory -Path $extractedRoot | Out-Null
    if ($isWindowsTarget) {
        Expand-Archive -LiteralPath $archive -DestinationPath $extractedRoot
    }
    else {
        & tar -xzf $archive -C $extractedRoot
        if ($LASTEXITCODE -ne 0) { throw 'Archive extraction failed' }
    }
    $extractedPackage = Join-Path $extractedRoot $packageName
    $executable = Join-Path $extractedPackage $binaryName
    $actualVersion = & $executable --version
    if ($LASTEXITCODE -ne 0 -or $actualVersion -ne "scarp $version") {
        throw 'Packaged version check failed'
    }
    $canonicalVersion = Invoke-ScarpProcess -Executable $executable -Arguments @('version')
    if ($canonicalVersion.ExitCode -ne 0 -or $canonicalVersion.Stderr.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($canonicalVersion.Stdout) -cne "scarp $version`n") { throw 'Packaged canonical version failed' }
    $diagnoseResult = Invoke-ScarpProcess -Executable $executable -Arguments @('diagnose', '--json')
    $hostReport = [System.Text.Encoding]::UTF8.GetString($diagnoseResult.Stdout) | ConvertFrom-Json -ErrorAction Stop
    $expectedHost = @{
        'x86_64-pc-windows-msvc' = @('windows', 'x86_64')
        'x86_64-unknown-linux-gnu' = @('linux', 'x86_64')
        'aarch64-apple-darwin' = @('macos', 'aarch64')
    }[$Target]
    if ($diagnoseResult.ExitCode -ne 0 -or $diagnoseResult.Stderr.Length -ne 0 -or $hostReport.schema_version -ne 1 -or $hostReport.version -cne $version -or $hostReport.host_os -cne $expectedHost[0] -or $hostReport.host_arch -cne $expectedHost[1] -or $hostReport.execution -cne 'interpreter' -or $hostReport.native_executable -ne $false -or $hostReport.package_manager -ne $false -or $hostReport.static_verify -ne $true -or $hostReport.module_inspection -ne $true) { throw 'Packaged diagnose differs from native host capabilities' }
    $canonicalRun = Invoke-ScarpProcess -Executable $executable -Arguments @('exec', (Join-Path $extractedPackage 'examples/hello.srp'))
    $legacyRun = Invoke-ScarpProcess -Executable $executable -Arguments @('run', (Join-Path $extractedPackage 'examples/hello.srp'))
    if ($canonicalRun.ExitCode -ne 0 -or $canonicalRun.Stderr.Length -ne 0 -or [System.Convert]::ToBase64String($canonicalRun.Stdout) -cne [System.Convert]::ToBase64String($legacyRun.Stdout)) { throw 'Packaged exec differs from run' }
    $staticSource = Join-Path $stagingRoot 'static-verification.srp'
    [System.IO.File]::WriteAllText($staticSource, 'print(1/0);')
    $staticCheck = Invoke-ScarpProcess -Executable $executable -Arguments @('verify', '--diagnostic-format=json', $staticSource)
    if ($staticCheck.ExitCode -ne 0 -or $staticCheck.Stdout.Length -ne 0 -or $staticCheck.Stderr.Length -ne 0) { throw 'Packaged verify must remain static and silent' }
    foreach ($navigationMode in @('symbols', 'references')) {
        $navigationResult = Invoke-ScarpProcess -Executable $executable -Arguments @('inspect', $navigationMode, '--json', (Join-Path $extractedPackage 'examples/symbol_navigation.srp'))
        $symbolInventory = [System.Text.Encoding]::UTF8.GetString($navigationResult.Stdout) | ConvertFrom-Json -ErrorAction Stop
        if ($navigationResult.ExitCode -ne 0 -or $navigationResult.Stderr.Length -ne 0 -or $symbolInventory.schema_version -ne 1 -or $symbolInventory.kind -cne "scarp.$navigationMode" -or $symbolInventory.symbols.Count -lt 2) { throw 'Packaged source navigation inventory failed' }
        if ($navigationMode -eq 'references' -and $symbolInventory.references.Count -eq 0) { throw 'Packaged reference inventory is empty' }
        $staticNavigation = Invoke-ScarpProcess -Executable $executable -Arguments @('inspect', $navigationMode, '--json', $staticSource)
        if ($staticNavigation.ExitCode -ne 0 -or $staticNavigation.Stderr.Length -ne 0) { throw 'Packaged navigation must not evaluate runtime arithmetic' }
    }

    if ($version -in @('0.0.5-alpha.4', '0.0.5-alpha.5', '0.0.6-alpha.1', '0.0.6-alpha.2', '0.0.6-alpha.3', '0.0.6-alpha.4', '0.0.6-alpha.5', '0.0.6-alpha.6', '0.0.6-alpha.7', '0.0.6-alpha.8', '0.0.6-alpha.9')) {
        # Exercise the actual extracted distributable, not a development binary.
        $projectPath = Join-Path $stagingRoot 'alpha4-project-smoke'
        $created = Invoke-ScarpProcess -Executable $executable -Arguments @('create', $projectPath)
        if ($created.ExitCode -ne 0 -or $created.Stderr.Length -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $projectPath 'Scarp.project'))) {
            throw 'Packaged Scarp project creation failed'
        }
        $projectVerify = Invoke-ScarpProcess -Executable $executable -Arguments @('verify', $projectPath)
        if ($projectVerify.ExitCode -ne 0 -or $projectVerify.Stdout.Length -ne 0 -or $projectVerify.Stderr.Length -ne 0) {
            throw 'Packaged Scarp project static verification failed'
        }
        if ($version -in @('0.0.5-alpha.5', '0.0.6-alpha.1', '0.0.6-alpha.2', '0.0.6-alpha.3', '0.0.6-alpha.4', '0.0.6-alpha.5', '0.0.6-alpha.6', '0.0.6-alpha.7', '0.0.6-alpha.8', '0.0.6-alpha.9')) {
            $projectModules = Invoke-ScarpProcess -Executable $executable -Arguments @('inspect', 'modules', '--json', $projectPath)
            if ($projectModules.ExitCode -ne 0 -or $projectModules.Stderr.Length -ne 0) {
                throw 'Packaged project directory module inspection failed'
            }
            $projectReport = [Text.Encoding]::UTF8.GetString($projectModules.Stdout) | ConvertFrom-Json -ErrorAction Stop
            if ($projectReport.schema_version -ne 1 -or $projectReport.modules.Count -lt 1) {
                throw 'Packaged project module JSON does not match schema 1'
            }
        }
        $projectLaunch = Invoke-ScarpProcess -Executable $executable -Arguments @('launch', $projectPath)
        if ($projectLaunch.ExitCode -ne 0 -or $projectLaunch.Stderr.Length -ne 0 -or [Text.Encoding]::UTF8.GetString($projectLaunch.Stdout) -cne "Hello from Scarp!`n") {
            throw 'Packaged Scarp project launch failed'
        }
        $projectEntry = Join-Path $projectPath 'src/main.srp'
        [IO.File]::WriteAllText($projectEntry, 'print(1/0);', [Text.UTF8Encoding]::new($false))
        $staticRuntime = Invoke-ScarpProcess -Executable $executable -Arguments @('verify', $projectPath)
        $executedRuntime = Invoke-ScarpProcess -Executable $executable -Arguments @('launch', $projectPath)
        if ($staticRuntime.ExitCode -ne 0 -or $staticRuntime.Stdout.Length -ne 0 -or $staticRuntime.Stderr.Length -ne 0 -or $executedRuntime.ExitCode -ne 1 -or $executedRuntime.Stdout.Length -ne 0) {
            throw 'Packaged project verify must not execute runtime arithmetic'
        }
        [IO.File]::WriteAllText((Join-Path $projectPath 'Scarp.project'), "scarp-project: 1`nname: alpha4-project-smoke`nentry: ../outside.srp`n", [Text.UTF8Encoding]::new($false))
        $invalid = Invoke-ScarpProcess -Executable $executable -Arguments @('verify', '--diagnostic-format=json', $projectPath)
        if ($invalid.ExitCode -ne 1 -or $invalid.Stdout.Length -ne 0) { throw 'Packaged project traversal rejected incorrectly' }
        $failure = [Text.Encoding]::UTF8.GetString($invalid.Stderr) | ConvertFrom-Json -ErrorAction Stop
        if ($failure.schema_version -ne 1) { throw 'Packaged project traversal JSON schema differs' }
    }
    if ($version -in @('0.0.6-alpha.8', '0.0.6-alpha.9')) {
        $modernProject = Join-Path $stagingRoot 'Scarp calculator smoke'
        $initialized = Invoke-ScarpProcess -Executable $executable -Arguments @('init', $modernProject)
        if ($initialized.ExitCode -ne 0 -or $initialized.Stderr.Length -ne 0) {
            throw 'Packaged declarative project initialization failed'
        }
        $example = Join-Path $extractedPackage 'examples/projects/calculator'
        foreach ($directory in @('src', 'tests')) {
            Get-ChildItem -LiteralPath (Join-Path $example $directory) -File | ForEach-Object {
                Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $modernProject $directory)
            }
        }
        $info = Invoke-ScarpProcess -Executable $executable -Arguments @('project', 'info', '--json', $modernProject)
        $projectMetadata = [Text.Encoding]::UTF8.GetString($info.Stdout) | ConvertFrom-Json -ErrorAction Stop
        if ($info.ExitCode -ne 0 -or $info.Stderr.Length -ne 0 -or $projectMetadata.schema_version -ne 1 -or $projectMetadata.manifest -cne 'scarp.project' -or $projectMetadata.format -cne 'v2' -or $projectMetadata.entry -cne 'src/main.srp') {
            throw 'Packaged project info schema or metadata differs'
        }
        $verified = Invoke-ScarpProcess -Executable $executable -Arguments @('verify', $modernProject)
        if ($verified.ExitCode -ne 0 -or $verified.Stdout.Length -ne 0 -or $verified.Stderr.Length -ne 0) {
            throw 'Packaged calculator static verification failed'
        }
        $executed = Invoke-ScarpProcess -Executable $executable -Arguments @('run', $modernProject)
        if ($executed.ExitCode -ne 0 -or $executed.Stderr.Length -ne 0 -or [Text.Encoding]::UTF8.GetString($executed.Stdout) -cne "5`n") {
            throw 'Packaged calculator execution failed'
        }
        $listed = Invoke-ScarpProcess -Executable $executable -Arguments @('test', '--json', '--list', '--require-tests', $modernProject)
        $discovery = [Text.Encoding]::UTF8.GetString($listed.Stdout) | ConvertFrom-Json -ErrorAction Stop
        if ($listed.ExitCode -ne 0 -or $listed.Stderr.Length -ne 0 -or $discovery.schema_version -ne 1 -or $discovery.mode -cne 'list' -or $discovery.discovered -ne 2 -or $discovery.tests[0].path -cne 'tests/addition.srp' -or $discovery.tests[1].path -cne 'tests/input.srp') {
            throw 'Packaged project test discovery failed'
        }
        $arguments = @('test', '--json', '--require-tests', '--filter', 'input', $modernProject)
        $tested = Invoke-ScarpProcess -Executable $executable -Arguments $arguments
        $suite = [Text.Encoding]::UTF8.GetString($tested.Stdout) | ConvertFrom-Json -ErrorAction Stop
        if ($tested.ExitCode -ne 0 -or $tested.Stderr.Length -ne 0 -or $suite.schema_version -ne 1 -or $suite.mode -cne 'run' -or $suite.discovered -ne 1 -or $suite.passed -ne 1 -or $suite.tests[0].path -cne 'tests/input.srp') {
            throw 'Packaged filtered fixture-backed test failed'
        }
        [IO.File]::WriteAllText((Join-Path $modernProject 'tests/input.stdin'), "17`n24`n", [Text.UTF8Encoding]::new($false))
        $failed = Invoke-ScarpProcess -Executable $executable -Arguments $arguments
        $failure = [Text.Encoding]::UTF8.GetString($failed.Stdout) | ConvertFrom-Json -ErrorAction Stop
        if ($failed.ExitCode -ne 1 -or $failed.Stderr.Length -ne 0 -or $failure.failed -ne 1 -or $failure.tests[0].error -notmatch 'E0065') {
            throw 'Packaged assertion failure did not report E0065'
        }
        if ($version -eq '0.0.6-alpha.9') {
            $structured = Invoke-ScarpProcess -Executable $executable -Arguments @('test', '--json=v2', '--require-tests', '--filter', 'input', $modernProject)
            $report = [Text.Encoding]::UTF8.GetString($structured.Stdout) | ConvertFrom-Json -ErrorAction Stop
            $case = $report.tests[0]
            if ($structured.ExitCode -ne 1 -or $structured.Stderr.Length -ne 0 -or $report.schema_version -ne 2 -or $report.failed -ne 1 -or $case.phase -cne 'runtime' -or $case.diagnostic.code -cne 'E0065' -or $case.diagnostic.primary.path -cne 'tests/input.srp') {
                throw 'Packaged structured test diagnostic differs'
            }
        }
    }
    $canonicalInspect = Invoke-ScarpProcess -Executable $executable -Arguments @('inspect', 'modules', '--json', '--diagnostic-format=json', (Join-Path $extractedPackage 'examples/hello.srp'))
    $inventory = [System.Text.Encoding]::UTF8.GetString($canonicalInspect.Stdout) | ConvertFrom-Json -ErrorAction Stop
    if ($canonicalInspect.ExitCode -ne 0 -or $canonicalInspect.Stderr.Length -ne 0 -or $inventory.schema_version -ne 1) { throw 'Packaged canonical module inspection failed' }
    $exampleSources = @(Get-ChildItem -LiteralPath (Join-Path $extractedPackage 'examples') -Filter '*.srp' -File | Sort-Object Name)
    if ($exampleSources.Count -eq 0) { throw 'Packaged example corpus is empty' }
    foreach ($exampleFile in $exampleSources) {
        $example = $exampleFile.BaseName
        $source = $exampleFile.FullName
        $inputPath = Join-Path $extractedPackage "examples/$example.stdin"
        $hasInput = Test-Path -LiteralPath $inputPath
        $inputBytes = if ($hasInput) { [IO.File]::ReadAllBytes($inputPath) } else { [byte[]]@() }
        $actual = Invoke-ScarpProcess -Executable $executable -Arguments @('run', $source) -InputBytes $inputBytes
        if ($actual.ExitCode -ne 0 -or $actual.Stderr.Length -ne 0) { throw "Packaged $example example failed" }
        $expected = [System.IO.File]::ReadAllBytes((Join-Path $extractedPackage "examples/$example.stdout"))
        if ([Convert]::ToBase64String($actual.Stdout) -cne [Convert]::ToBase64String($expected)) { throw "Packaged $example output differs" }
        $checkVerb = if ($hasInput) { 'verify' } else { 'check' }
        $checked = Invoke-ScarpProcess -Executable $executable -Arguments @($checkVerb, $source)
        if ($checked.ExitCode -ne 0 -or $checked.Stdout.Length -ne 0 -or $checked.Stderr.Length -ne 0) { throw "Packaged $example validation failed" }
        $inventory = Invoke-ScarpProcess -Executable $executable -Arguments @('modules', $source)
        if ($inventory.ExitCode -ne 0 -or $inventory.Stderr.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($inventory.Stdout).StartsWith('modules: ')) {
            throw "Packaged $example static module inspection failed"
        }
        $json = Invoke-ScarpProcess -Executable $executable -Arguments @('modules', '--json', $source)
        if ($json.ExitCode -ne 0 -or $json.Stderr.Length -ne 0) { throw "Packaged $example JSON inspection failed" }
        $metadata = [System.Text.Encoding]::UTF8.GetString($json.Stdout) | ConvertFrom-Json -AsHashtable
        $expectedSchema = if ($example -eq 'fixed_arrays') { 3 }
        elseif ($example -in @('tuples', 'core_input_tuple')) { 2 }
        else { 1 }
        if ($metadata.schema_version -ne $expectedSchema -or $metadata.entry -ne 0 -or $metadata.modules.Count -lt 1) {
            throw "Packaged $example JSON schema failed"
        }
    }
    $explanation = Invoke-ScarpProcess -Executable $executable -Arguments @('explain', 'E0044')
    if ($explanation.ExitCode -ne 0 -or $explanation.Stderr.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($explanation.Stdout).StartsWith('E0044: ')) {
        throw 'Packaged diagnostic explanation failed'
    }
    $moduleEntry = Join-Path $stagingRoot 'module-main.srp'
    $moduleLibrary = Join-Path $stagingRoot 'module-library.srp'
    [System.IO.File]::WriteAllText($moduleEntry, 'import "module-library.srp" as lib; print("OUT"); eprint("ERR"); print(lib.secret);')
    foreach ($case in @(@('bind secret=7;', 'E0041'), @('import "module-main.srp" as entry;', 'E0039'))) {
        [System.IO.File]::WriteAllText($moduleLibrary, $case[0])
        foreach ($verb in @('run', 'check', 'modules')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, $moduleEntry)
            $diagnostic = [System.Text.Encoding]::UTF8.GetString($failed.Stderr)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not $diagnostic.StartsWith(('error[' + $case[1] + ']')) -or -not $diagnostic.Contains('module-library.srp')) {
                throw 'Packaged module visibility/cycle diagnostic or stream atomicity failed'
            }
        }
    }
    [System.IO.File]::WriteAllText($moduleEntry, 'import "module-library.srp" as lib; print(lib.fail());')
    [System.IO.File]::WriteAllText($moduleLibrary, 'pub pure funcra fail(n: integer=1/0)->integer needs false; {return n;}')
    $staticInventory = Invoke-ScarpProcess -Executable $executable -Arguments @('modules', $moduleEntry)
    if ($staticInventory.ExitCode -ne 0 -or $staticInventory.Stderr.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($staticInventory.Stdout).Contains('pub pure funcra fail(n: integer = <default>) -> integer')) {
        throw 'Packaged module inspection must not execute defaults or contracts'
    }
    [System.IO.File]::WriteAllText($moduleEntry, 'import "module-library.srp" as lib; print(lib.value);')
    foreach ($case in @(
        @('pub bind value=1/0;', 'E0008'),
        @('pub bind value=9223372036854775807+1;', 'E0007'),
        @('pub bind value=1+true;', 'E0015'),
        @('pub bind value: float=1;', 'E0015'),
        @('pub bind value: number=1;', 'E0004'),
        @('pub bind value=value;', 'E0039'),
        @('pub bind value=later; bind later=1;', 'E0042'),
        @('pub bind value=true or (sqrt(4.0)>0.0);', 'E0037')
    )) {
        [System.IO.File]::WriteAllText($moduleLibrary, $case[0])
        foreach ($verb in @('run', 'check', 'modules')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, $moduleEntry)
            $diagnostic = [System.Text.Encoding]::UTF8.GetString($failed.Stderr)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not $diagnostic.StartsWith(('error[' + $case[1] + ']')) -or -not $diagnostic.Contains('module-library.srp')) {
                throw 'Packaged constant compilation diagnostic or stream atomicity failed'
            }
        }
    }
    [System.IO.File]::WriteAllText($moduleEntry, 'import "absent-module.srp" as absent;')
    $moduleTokens = Invoke-ScarpProcess -Executable $executable -Arguments @('tokens', $moduleEntry)
    if ($moduleTokens.ExitCode -ne 0 -or $moduleTokens.Stderr.Length -ne 0) {
        throw 'Packaged tokens must not follow imports'
    }
    $scalarSource = Join-Path $stagingRoot 'scalar-text.srp'
    foreach ($case in @(
        @('print(to_float(9223372036854775807));', 'E0044'),
        @('print(to_integer(1.5));', 'E0044'),
        @('print(text_codepoint("a", 1));', 'E0045')
    )) {
        [System.IO.File]::WriteAllText($scalarSource, $case[0])
        foreach ($verb in @('run', 'check')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, $scalarSource)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($failed.Stderr).StartsWith(('error[' + $case[1] + ']'))) {
                throw 'Packaged scalar/text domain diagnostic or atomic output failed'
            }
        }
        $static = Invoke-ScarpProcess -Executable $executable -Arguments @('modules', $scalarSource)
        if ($static.ExitCode -ne 0 -or $static.Stderr.Length -ne 0) {
            throw 'Module inspection must not execute scalar/text runtime calls'
        }
    }
    $streamSource = Join-Path $stagingRoot 'streams.srp'
    [System.IO.File]::WriteAllText($streamSource, 'bind word="ready"; write(word, true,); eprint("warning", 2); ewrite("tail");')
    $streams = Invoke-ScarpProcess -Executable $executable -Arguments @('run', $streamSource)
    if ($streams.ExitCode -ne 0 -or [System.Text.Encoding]::UTF8.GetString($streams.Stdout) -cne 'ready true' -or [System.Text.Encoding]::UTF8.GetString($streams.Stderr) -cne "warning 2`ntail") {
        throw 'Packaged explicit output stream check failed'
    }
    $silent = Invoke-ScarpProcess -Executable $executable -Arguments @('check', $streamSource)
    if ($silent.ExitCode -ne 0 -or $silent.Stdout.Length -ne 0 -or $silent.Stderr.Length -ne 0) {
        throw 'Packaged dual-stream validation must be silent'
    }
    $invalidSource = Join-Path $stagingRoot 'invalid.srp'
    $budgetSource = Join-Path $stagingRoot 'budget.srp'
    [System.IO.File]::WriteAllText($budgetSource, 'println("hidden"); repeat 9223372036854775807 as i { continue; }')
    foreach ($verb in @('run', 'check')) {
        $limited = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, '--max-steps', '20', $budgetSource)
        if ($limited.ExitCode -ne 1 -or $limited.Stdout.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($limited.Stderr) -notmatch 'E0017') {
            throw 'Packaged counted-loop budget or atomicity check failed'
        }
        $guidance = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, '--help')
        if ($guidance.ExitCode -ne 0 -or $guidance.Stderr.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($guidance.Stdout) -notmatch '--max-steps') {
            throw 'Packaged budget help failed'
        }
    }
    $errorLog = Join-Path $stagingRoot 'invalid.stderr'
    $functionSource = Join-Path $stagingRoot 'function-budgets.srp'
    $functionBody = 'pure funcra f(n: integer) -> integer { if n == 0 { return 0; } return f(n - 1); } '
    [System.IO.File]::WriteAllText($functionSource, $functionBody + 'print(f(4));')
    foreach ($verb in @('run', 'check')) {
        $bounded = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, '--max-call-depth', '5', '--max-calls', '5', '--max-live-slots', '10', '--max-capture-bytes', '72', $functionSource)
        if ($bounded.ExitCode -ne 0 -or $bounded.Stderr.Length -ne 0) {
            throw 'Packaged function budget boundary check failed'
        }
        if ($verb -eq 'run') {
            if ([Convert]::ToBase64String($bounded.Stdout) -cne [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes("0`n"))) {
                throw 'Packaged bounded function output differs'
            }
        }
        elseif ($bounded.Stdout.Length -ne 0) { throw 'Packaged bounded validation must be silent' }
    }
    [System.IO.File]::WriteAllText($functionSource, $functionBody + 'print("OUT"); eprint("ERR"); print(f(4));')
    foreach ($verb in @('run', 'check')) {
        foreach ($flag in @('--max-call-depth', '--max-calls', '--max-live-slots', '--max-capture-bytes')) {
            $limited = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, $flag, '0', $functionSource)
            $diagnostic = [System.Text.Encoding]::UTF8.GetString($limited.Stderr)
            $code = if ($flag -eq '--max-capture-bytes') { 'E0029' } else { 'E0027' }
            if ($limited.ExitCode -ne 1 -or $limited.Stdout.Length -ne 0 -or -not $diagnostic.StartsWith("error[$code]") -or -not $diagnostic.Contains($flag)) {
                throw "Packaged function resource/atomicity check failed: $flag"
            }
        }
    }
    [System.IO.File]::WriteAllText($invalidSource, 'println("first"); println(1 / 0);')
    $invalidOutput = & $executable run $invalidSource 2> $errorLog
    if ($LASTEXITCODE -ne 1 -or $invalidOutput) { throw 'Packaged error exit/output check failed' }
    if ([System.IO.File]::ReadAllText($errorLog) -notmatch 'E0008') {
        throw 'Packaged arithmetic diagnostic check failed'
    }
    $floatSource = Join-Path $stagingRoot 'floats.srp'
    [System.IO.File]::WriteAllText($floatSource, 'pure funcra half(n: float = 1.0) -> float { return n / 2.0; } print(half()); eprint(-0.0);')
    foreach ($verb in @('run', 'check')) {
        $floats = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, '--max-steps', '100', '--max-prints', '2', '--max-call-depth', '1', '--max-calls', '1', '--max-live-slots', '2', '--max-capture-bytes', '144', $floatSource)
        if ($floats.ExitCode -ne 0) { throw 'Packaged float call/resource check failed' }
        if ($verb -eq 'run') {
            if ([System.Text.Encoding]::UTF8.GetString($floats.Stdout) -cne "0.5`n" -or [System.Text.Encoding]::UTF8.GetString($floats.Stderr) -cne "-0.0`n") {
                throw 'Packaged float streams or signed-zero formatting differ'
            }
        }
        elseif ($floats.Stdout.Length -ne 0 -or $floats.Stderr.Length -ne 0) {
            throw 'Packaged float validation must be silent'
        }
    }
    foreach ($case in @(@('1e309', 'E0030'), @('1e308 * 2.0', 'E0031'), @('1.0 / -0.0', 'E0008'))) {
        [System.IO.File]::WriteAllText($floatSource, ('print("OUT"); eprint("ERR"); print(' + $case[0] + ');'))
        foreach ($verb in @('run', 'check')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb, $floatSource)
            $diagnostic = [System.Text.Encoding]::UTF8.GetString($failed.Stderr)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not $diagnostic.StartsWith(('error[' + $case[1] + ']'))) {
                throw 'Packaged float failure must discard both program streams'
            }
        }
    }
    $vectorSource = Join-Path $stagingRoot 'vector-smoke.srp'
    [System.IO.File]::WriteAllText($vectorSource, 'pure funcra f(v: vector=fill(0.5,2)) -> vector { return add(v,v); } print(f()); eprint(dot(f(),[2.0,3.0]));')
    $vectorFlags = @('--max-steps','200','--max-prints','2','--max-call-depth','1','--max-calls','2','--max-live-slots','2','--max-capture-bytes','152')
    $vectorRun = Invoke-ScarpProcess -Executable $executable -Arguments (@('run') + $vectorFlags + @($vectorSource))
    if ($vectorRun.ExitCode -ne 0 -or [System.Text.Encoding]::UTF8.GetString($vectorRun.Stdout) -cne "[1.0, 1.0]`n" -or [System.Text.Encoding]::UTF8.GetString($vectorRun.Stderr) -cne "5.0`n") {
        throw 'Packaged vector calls or exact streams failed'
    }
    $vectorCheck = Invoke-ScarpProcess -Executable $executable -Arguments (@('check') + $vectorFlags + @($vectorSource))
    if ($vectorCheck.ExitCode -ne 0 -or $vectorCheck.Stdout.Length -ne 0 -or $vectorCheck.Stderr.Length -ne 0) {
        throw 'Packaged vector check must be silent'
    }
    foreach ($case in @(@('dot([], [1.0])','E0032'), @('[1.0][-1]','E0033'), @('mean([])','E0034'), @('scale([1e308],2.0)','E0031'))) {
        [System.IO.File]::WriteAllText($vectorSource, ('print("OUT"); eprint("ERR"); print(' + $case[0] + ');'))
        foreach ($verb in @('run','check')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,$vectorSource)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($failed.Stderr).StartsWith(('error[' + $case[1] + ']'))) {
                throw 'Packaged vector diagnostic or atomicity failed'
            }
        }
    }
    [System.IO.File]::WriteAllText($vectorSource, 'print([1.0,2.0]); eprint([3.0]);')
    foreach ($verb in @('run','check')) {
        $limited = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-capture-bytes','79',$vectorSource)
        if ($limited.ExitCode -ne 1 -or $limited.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($limited.Stderr).StartsWith('error[E0029]')) {
            throw 'Packaged vector snapshots must obey the capture cap'
        }
    }
    $integerSource = Join-Path $stagingRoot 'integer-collections.srp'
    [System.IO.File]::WriteAllText($integerSource, 'bind a=[integer:1,2,3]; bind b=ints_window(a,1,2); print(ints_set(a,0,9),b,ints_sum(b));')
    foreach ($verb in @('run','check')) {
        $result = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-vector-bytes','48',$integerSource)
        if ($result.ExitCode -ne 0 -or $result.Stderr.Length -ne 0) { throw 'Packaged integer collection smoke failed' }
        $expected = if ($verb -eq 'run') { "[integer: 9, 2, 3] [integer: 2, 3] 5`n" } else { '' }
        if ([System.Text.Encoding]::UTF8.GetString($result.Stdout) -cne $expected) { throw 'Packaged integer collection output differs' }
        $limited = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-vector-bytes','47',$integerSource)
        if ($limited.ExitCode -ne 1 -or $limited.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($limited.Stderr).StartsWith('error[E0036]')) { throw 'Packaged integer allocation cap failed' }
    }
    foreach ($case in @(@('ints_sum([integer:9223372036854775807,1])','E0007'), @('ints_window([integer:1],1,1)','E0033'), @('ints_from_vector([0.5])','E0044'))) {
        [System.IO.File]::WriteAllText($integerSource, ('print("OUT"); eprint("ERR"); print(' + $case[0] + ');'))
        foreach ($verb in @('run','check')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,$integerSource)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($failed.Stderr).StartsWith(('error[' + $case[1] + ']'))) { throw 'Packaged integer diagnostic or atomicity failed' }
        }
    }
    $textSource = Join-Path $stagingRoot 'bounded-text.srp'
    [System.IO.File]::WriteAllText($textSource, 'bind s=text_concat("é","🌍"); print(text_repeat(text_slice(s,1,1),2));')
    foreach ($verb in @('run','check')) {
        $result = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-text-bytes','24',$textSource)
        $expected = if ($verb -eq 'run') { "🌍🌍`n" } else { '' }
        if ($result.ExitCode -ne 0 -or $result.Stderr.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($result.Stdout) -cne $expected) { throw 'Packaged Unicode text construction failed' }
        $limited = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-text-bytes','23',$textSource)
        if ($limited.ExitCode -ne 1 -or $limited.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($limited.Stderr).StartsWith('error[E0048]')) { throw 'Packaged text allocation cap failed' }
    }
    $structured = Invoke-ScarpProcess -Executable $executable -Arguments @('check','--diagnostic-format=json','--max-text-bytes','23',$textSource)
    $diagnostic = [System.Text.Encoding]::UTF8.GetString($structured.Stderr) | ConvertFrom-Json -ErrorAction Stop
    if ($structured.ExitCode -ne 1 -or $structured.Stdout.Length -ne 0 -or $diagnostic.schema_version -ne 1 -or $diagnostic.code -cne 'E0048' -or $diagnostic.column_encoding -cne 'unicode-scalar' -or $diagnostic.primary.span.Count -ne 2) { throw 'Packaged structured text diagnostic failed' }
    $bitsSource = Join-Path $stagingRoot 'bits.srp'
    [System.IO.File]::WriteAllText($bitsSource, 'var flags=0xF0; flags|=0b11; print(flags&15,1<<63,-8>>1,-1>>>1);')
    foreach ($verb in @('run','check')) {
        $bits = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,$bitsSource)
        $expected = if ($verb -eq 'run') { "3 -9223372036854775808 -4 9223372036854775807`n" } else { '' }
        if ($bits.ExitCode -ne 0 -or $bits.Stderr.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($bits.Stdout) -cne $expected) { throw 'Packaged integer bit operations failed' }
    }
    foreach ($case in @(@('1<<64','E0046'), @('0b102','E0006'), @('1.0&2.0','E0015'), @('-9223372036854775808%-1','E0007'), @('1%0','E0008'))) {
        [System.IO.File]::WriteAllText($bitsSource, ('print("OUT"); eprint("ERR"); print(' + $case[0] + ');'))
        foreach ($verb in @('run','check')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,$bitsSource)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($failed.Stderr).StartsWith(('error[' + $case[1] + ']'))) { throw 'Packaged integer bit diagnostic or atomicity failed' }
        }
    }
    $kernelSource = Join-Path $stagingRoot 'kernels.srp'
    [System.IO.File]::WriteAllText($kernelSource, 'bind v=[1.0,3.0]; print(variance(v),stddev(v),sigmoid([-1000.0,0.0,1000.0]),relu([-1.0,0.0,1.0]),tanh([0.0]));')
    foreach ($verb in @('run','check')) {
        $kernel = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,$kernelSource)
        $expected = if ($verb -eq 'run') { "1.0 1.0 [0.0, 0.5, 1.0] [0.0, 0.0, 1.0] [0.0]`n" } else { '' }
        if ($kernel.ExitCode -ne 0 -or $kernel.Stderr.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($kernel.Stdout) -cne $expected) { throw 'Packaged population statistics or activations failed' }
    }
    foreach ($case in @(@('variance([])','E0034'), @('variance([1e308,-1e308])','E0031'), @('sigmoid([integer:1])','E0015'))) {
        [System.IO.File]::WriteAllText($kernelSource, ('print("OUT"); eprint("ERR"); print(' + $case[0] + ');'))
        foreach ($verb in @('run','check')) {
            $failed = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,$kernelSource)
            if ($failed.ExitCode -ne 1 -or $failed.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($failed.Stderr).StartsWith(('error[' + $case[1] + ']'))) { throw 'Packaged statistics diagnostic or atomicity failed' }
        }
    }
    $rmsSource = Join-Path $stagingRoot 'rms.srp'
    [System.IO.File]::WriteAllText($rmsSource, 'print(rms([1e308,-1e308])==1e308);')
    foreach ($verb in @('exec','check')) {
        $result = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-vector-bytes','16',$rmsSource)
        $expected = if ($verb -eq 'exec') { "true`n" } else { '' }
        if ($result.ExitCode -ne 0 -or $result.Stderr.Length -ne 0 -or [System.Text.Encoding]::UTF8.GetString($result.Stdout) -cne $expected) { throw 'Packaged finite RMS or allocation accounting failed' }
        $limited = Invoke-ScarpProcess -Executable $executable -Arguments @($verb,'--max-vector-bytes','15',$rmsSource)
        if ($limited.ExitCode -ne 1 -or $limited.Stdout.Length -ne 0 -or -not [System.Text.Encoding]::UTF8.GetString($limited.Stderr).StartsWith('error[E0036]')) { throw 'Packaged RMS input allocation cap failed' }
    }
    [System.IO.File]::WriteAllText($rmsSource, 'print("OUT"); eprint("ERR"); print(rms([]));')
    $rmsError = Invoke-ScarpProcess -Executable $executable -Arguments @('exec','--diagnostic-format=json',$rmsSource)
    $rmsDiagnostic = [System.Text.Encoding]::UTF8.GetString($rmsError.Stderr) | ConvertFrom-Json -ErrorAction Stop
    if ($rmsError.ExitCode -ne 1 -or $rmsError.Stdout.Length -ne 0 -or $rmsDiagnostic.code -cne 'E0034') { throw 'Packaged RMS domain failure must discard program streams' }
    $rmsStatic = Invoke-ScarpProcess -Executable $executable -Arguments @('verify',$rmsSource)
    if ($rmsStatic.ExitCode -ne 0 -or $rmsStatic.Stdout.Length -ne 0 -or $rmsStatic.Stderr.Length -ne 0) { throw 'Packaged RMS static verification must not evaluate calls' }
    $tokens = & $executable tokens (Join-Path $extractedPackage 'examples/arithmetic.srp')
    if ($LASTEXITCODE -ne 0 -or -not ($tokens -match 'integer\(1\)')) {
        throw 'Packaged token inspection failed'
    }

    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksum = "$hash  $([System.IO.Path]::GetFileName($archive))`n"
    [System.IO.File]::WriteAllText(($archive + '.sha256'), $checksum, [System.Text.UTF8Encoding]::new($false))
    Write-Host "Verified package: $archive"
    Write-Host "SHA-256: $hash"
}
finally {
    Pop-Location
}
