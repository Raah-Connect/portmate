#!/usr/bin/env pwsh
<#
winclick.ps1

Windows PowerShell client for piping Hoon/threads into a running Urbit ship.

This draft:
- accepts a -PipeName argument
- uses NamedPipeClientStream
- integrates with winclick-format.ps1
- supports jam-only, execute, and full flow modes

NOTE:
The exact conn.c message framing may need refinement for your specific Urbit setup.
#>

$ErrorActionPreference = "Stop"

# ==========================================================
# DEFAULTS / STATE
# ==========================================================

$EXECUTE      = $false
$FILTER_GOOF  = $false
$JAM_HEX      = $false
$JAM_ONLY     = $false
$KHAN_TED     = $false
$CARD         = $false

$EVAL_CMD     = $null
$INPUT        = $null
$OUTPUT       = $null
$PIPE_NAME    = $null

$POSITIONALS  = New-Object System.Collections.Generic.List[string]

# ==========================================================
# FUNCTIONS
# ==========================================================

function Show-Help {
    $scriptName = Split-Path -Leaf $PSCommandPath
@"
Usage:
    $scriptName [options] <hoon> [<dependencies> ...]
    $scriptName [options] -i <path-to-file> <hoon> [<dependencies> ...]
    $scriptName [-o|-p] -e -i <path-to-file>

Thin client for interacting with a running Urbit ship via a Windows named pipe.

options:
    -b <path-to-eval>   Specify external eval executable/script
    -c                  Send a conn.c card to urbit ship
    -e                  Execute jammed Hoon
    -h                  Show usage info
    -i <path-to-file>   Read input from file
    -j                  Jam only
    -k                  Execute command using "khan-eval" thread
    -o <path-to-file>   Output to file
    -p                  Filter failure stack traces from result and pretty-print them to stderr
    -x                  Jam to hex
    -PipeName <name>    Windows named pipe endpoint (default: autodetect or set explicitly)
"@
}

function Escape-SingleQuotes([string]$s) {
    return ($s -replace "'", "''")
}

function Invoke-WinclickFormat {
    param(
        [string]$HoonText,
        [string[]]$Dependencies,
        [switch]$Khan
    )

    $fmt = @()
    $fmt += "winclick-format.ps1"
    if ($Khan) { $fmt += "-k" }
    $fmt += $HoonText
    if ($Dependencies) { $fmt += $Dependencies }

    & @fmt
}

function Invoke-ShipTransport {
    param(
        [Parameter(Mandatory=$true)]
        [string]$PipeName,

        [Parameter(Mandatory=$true)]
        [string]$Payload
    )

    $client = [System.IO.Pipes.NamedPipeClientStream]::new(
        ".",
        $PipeName,
        [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None
    )

    try {
        $client.Connect(5000)

        $writer = [System.IO.StreamWriter]::new($client)
        $writer.AutoFlush = $true

        $reader = [System.IO.StreamReader]::new($client)

        # Write payload
        $writer.Write($Payload)
        $writer.Flush()

        # Try to read response
        $response = $reader.ReadToEnd()
        return $response
    }
    finally {
        $client.Dispose()
    }
}

function Get-JammedPayload {
    param(
        [string]$FormattedHoon
    )

    if (-not $EVAL_CMD) {
        # Default to using winclick-format only if no eval binary is provided.
        # You should set -b to your actual jam/cue-capable executable if needed.
        return $FormattedHoon
    }

    # If your eval tool accepts stdin/stdout like:
    #   <payload> | eval --jam -n
    # you'd need to adapt this section to your actual executable.
    #
    # For now, we assume the eval command can be invoked directly with the
    # formatted Hoon as an argument or via stdin.
    $tempIn = [System.IO.Path]::GetTempFileName()
    $tempOut = [System.IO.Path]::GetTempFileName()

    try {
        Set-Content -LiteralPath $tempIn -Value $FormattedHoon -NoNewline

        $args = @("eval", "--jam", "-n")
        if ($JAM_HEX) {
            # If your evaluator supports hex output, wire it here.
        }

        $result = & $EVAL_CMD @args < $tempIn
        return ($result | Out-String).TrimEnd()
    }
    finally {
        Remove-Item -Force -ErrorAction SilentlyContinue $tempIn, $tempOut
    }
}

function Read-InputFile {
    param([string]$Path)
    return Get-Content -Raw -LiteralPath $Path
}

# ==========================================================
# ARGUMENT PARSING
# ==========================================================

for ($i = 0; $i -lt $args.Count; $i++) {
    $arg = $args[$i]

    switch ($arg) {
        "-b" {
            $i++
            if ($i -ge $args.Count) { throw "Missing value for -b" }
            $EVAL_CMD = $args[$i]
        }
        "-e" { $EXECUTE = $true }
        "-i" {
            $i++
            if ($i -ge $args.Count) { throw "Missing value for -i" }
            $INPUT = $args[$i]
        }
        "-j" { $JAM_ONLY = $true }
        "-k" { $KHAN_TED = $true }
        "-o" {
            $i++
            if ($i -ge $args.Count) { throw "Missing value for -o" }
            $OUTPUT = $args[$i]
        }
        "-p" { $FILTER_GOOF = $true }
        "-x" { $JAM_HEX = $true }
        "-c" { $CARD = $true }
        "-h" {
            Show-Help
            exit 0
        }
        "-PipeName" {
            $i++
            if ($i -ge $args.Count) { throw "Missing value for -PipeName" }
            $PIPE_NAME = $args[$i]
        }
        default {
            if ($arg.StartsWith("-")) {
                throw "Invalid option: $arg"
            }
            $POSITIONALS.Add($arg)
        }
    }
}

# ==========================================================
# VALIDATION
# ==========================================================

if ($EXECUTE) {
    if ($JAM_ONLY) {
        throw "Invalid option: cannot mix -e and -j"
    }
    if (-not $INPUT) {
        throw "Invalid option: -e requires -i"
    }
    if ($CARD) {
        Write-Warning "-c meaningless with -e; ignoring"
    }
    if ($KHAN_TED) {
        Write-Warning "-k meaningless with -e; ignoring"
    }
}

if ($KHAN_TED -and $CARD) {
    Write-Warning "-k meaningless with -c; ignoring"
}

if ($JAM_ONLY) {
    if ($FILTER_GOOF) {
        Write-Warning "-p meaningless with -j; ignoring"
    }
}
elseif ($JAM_HEX) {
    throw "Invalid option: -x requires -j"
}

if (-not $PIPE_NAME) {
    throw "Missing required -PipeName argument"
}

if (-not $EVAL_CMD) {
    # Default to the formatter if you have no separate evaluator yet.
    # Replace this with your actual jam/cue-capable tool.
    $EVAL_CMD = "winclick-eval.ps1"
}

# ==========================================================
# ARGUMENT RESOLUTION
# ==========================================================

if ($INPUT) {
    $minInput = 1
}
else {
    $minInput = 2
}

if ($POSITIONALS.Count -lt $minInput) {
    Write-Host "ERROR: missing input" -ForegroundColor Red
    Show-Help
    exit 1
}

$HOON  = $null
$PIER  = $null
$DEPS  = @()

if ($INPUT) {
    $PIER = $POSITIONALS[0]

    if (-not (Test-Path -LiteralPath $INPUT)) {
        throw "Input file not found: $INPUT"
    }

    if (-not $EXECUTE) {
        # Read file, escape single quotes, flatten newlines into spaces
        $raw = Get-Content -Raw -LiteralPath $INPUT
        $HOON = ($raw -replace "'", "''") -replace "`r?`n", " "
    }
}
else {
    $PIER = $POSITIONALS[0]
    $HOON = $POSITIONALS[1]
    if ($POSITIONALS.Count -gt 2) {
        $DEPS = $POSITIONALS | Select-Object -Skip 2
    }
}

# ==========================================================
# MODE SETUP
# ==========================================================

if ($KHAN_TED) {
    # Formatting is done with -k
    $formatterKhan = $true
}
else {
    $formatterKhan = $false
}

if ($CARD) {
    $DEPS = @()
}

# ==========================================================
# MAIN PIPELINE
# ==========================================================

$tmpOut = [System.IO.Path]::GetTempFileName()

try {
    if ($EXECUTE) {
        # Execute pre-jammed input from file
        $payload = Get-Content -Raw -LiteralPath $INPUT
        $response = Invoke-ShipTransport -PipeName $PIPE_NAME -Payload $payload
    }
    elseif ($JAM_ONLY) {
        $formatted = Invoke-WinclickFormat -HoonText $HOON -Dependencies $DEPS -Khan:$formatterKhan
        $response = $formatted
    }
    else {
        $formatted = Invoke-WinclickFormat -HoonText $HOON -Dependencies $DEPS -Khan:$formatterKhan

        # Jam step
        $jammed = Get-JammedPayload -FormattedHoon $formatted

        # Send jammed payload to ship
        $response = Invoke-ShipTransport -PipeName $PIPE_NAME -Payload $jammed
    }

    # Optional filter mode placeholder
    if ($FILTER_GOOF) {
        # If you have a failure-stack filter, apply it here.
        # For now this is a passthrough.
        $response = $response
    }

    Set-Content -LiteralPath $tmpOut -Value $response -NoNewline

    if ($OUTPUT) {
        Move-Item -Force -LiteralPath $tmpOut -Destination $OUTPUT
        $tmpOut = $null
    }
    else {
        Get-Content -Raw -LiteralPath $tmpOut
    }
}
finally {
    if ($tmpOut -and (Test-Path -LiteralPath $tmpOut)) {
        Remove-Item -Force -LiteralPath $tmpOut
    }
}