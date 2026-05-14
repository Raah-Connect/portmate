#!/usr/bin/env pwsh
<#
winclick-format.ps1

Formats a Hoon command and optional dependencies as a Hoon noun suitable for
Urbit conn.c / eval threads.
#>

$ErrorActionPreference = "Stop"

# ==========================================================
# CONSTANTS
# ==========================================================

$RID       = 0
$COMMAND   = "%fyrd"
$DESK      = "%base"

$EVAL_TED  = "%eval"
$KHAN_TED  = "%khan-eval"

$MARK_OUT  = "%noun"
$MARK_IN   = "%ted-eval"

# ==========================================================
# FUNCTIONS
# ==========================================================

function Show-Help {
    $scriptName = Split-Path -Leaf $PSCommandPath
@"
Usage:
    $scriptName [options] <hoon> [<dependency> ...]

Formats a text representation of a Hoon noun for use as input to eval threads
via conn.c.

options:
    -h  Show usage info
    -k  Format for the 'khan-eval' thread instead
"@ | Write-Host
}

function Quote-HoonAtom {
    param([Parameter(Mandatory=$true)][string]$Text)

    # The Bash script wraps strings in single quotes and escapes internal quotes.
    # We mimic that behavior explicitly.
    $escaped = $Text -replace "'", "''"
    return "'$escaped'"
}

function Build-TedInput {
    param(
        [Parameter(Mandatory=$true)]
        [string]$Hoon,

        [string[]]$Dependencies
    )

    if (-not $Dependencies -or $Dependencies.Count -eq 0) {
        return "[$MARK_IN '$Hoon']"
    }

    # Mirror the Bash structure more carefully.
    #
    # Bash original intent:
    #   TED_IN="'$1'"
    #   shift
    #   TED_IN="[$MARK_IN [$TED_IN [$* ~]]]"
    #
    # This means:
    #   - first arg becomes the Hoon atom
    #   - remaining args become a list terminated by ~
    #
    # We reproduce that structure here using explicit joining.
    $depsText = ($Dependencies | ForEach-Object { $_ }) -join ' '
    $quotedHoon = Quote-HoonAtom $Hoon

    return "[$MARK_IN [$quotedHoon [$depsText ~]]]"
}

# ==========================================================
# MAIN
# ==========================================================

$THREAD = $EVAL_TED
$ARGS_OUT = New-Object System.Collections.Generic.List[string]

for ($i = 0; $i -lt $args.Count; $i++) {
    $arg = $args[$i]
    switch ($arg) {
        "-h" {
            Show-Help
            exit 0
        }
        "-k" {
            $THREAD = $KHAN_TED
        }
        default {
            if ($arg.StartsWith("-")) {
                Write-Host "Invalid option: $arg" -ForegroundColor Red
                exit 1
            }
            $ARGS_OUT.Add($arg)
        }
    }
}

if ($ARGS_OUT.Count -lt 1) {
    Write-Host "ERROR: no command" -ForegroundColor Red
    Show-Help
    exit 1
}

$HOON = $ARGS_OUT[0]
$DEPS = @()
if ($ARGS_OUT.Count -gt 1) {
    $DEPS = $ARGS_OUT | Select-Object -Skip 1
}

$TED_IN = Build-TedInput -Hoon $HOON -Dependencies $DEPS

# Final conn.c thread noun
Write-Output "[$RID $COMMAND [$DESK $THREAD $MARK_OUT $TED_IN]]"