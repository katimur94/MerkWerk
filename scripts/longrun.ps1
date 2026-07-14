#Requires -Version 5.0
<#
.SYNOPSIS
    MerkWerk Langzeit-Ressourcentest — 8-Stunden-Stabilitätsprüfung für Daemon.

.DESCRIPTION
    Sampelt periodisch (default: 30s Intervall) Ressourcenmetriken des
    merkwerk-daemon-Prozesses über X Stunden (default: 8h). Exportiert zu CSV,
    berechnet Statistiken, warnt vor Speicherlecks (Regressions-Heuristik).

    Setzt NICHT den Daemon selbst auf; der Prozess muss bereits laufen.
    Endet mit PASS/FAIL gegen Schwellen: <5% avg CPU, <200 MB max RAM.

.PARAMETER DurationHours
    Test-Dauer in Stunden (default: 8).

.PARAMETER ProcessName
    Name des zu überwachenden Prozesses (default: "merkwerk-daemon").

.PARAMETER SampleIntervalSec
    Sampling-Intervall in Sekunden (default: 30).

.PARAMETER OutCsv
    Pfad der CSV-Ausgabedatei (default: "longrun.csv").

.EXAMPLE
    .\longrun.ps1 -DurationHours 8 -ProcessName "merkwerk-daemon" -SampleIntervalSec 30 -OutCsv "results.csv"

.NOTES
    Definition of Done (aus ARCHITEKTUR.md):
    - Daemon-CPU: < 5% durchschnittlich über 8 Stunden
    - Daemon-RAM: < 200 MB Peak
    - Kein Speicherleck erkannt (lineare Regression WorkingSet)
#>

param (
    [int] $DurationHours = 8,
    [string] $ProcessName = "merkwerk-daemon",
    [int] $SampleIntervalSec = 30,
    [string] $OutCsv = "longrun.csv"
)

# ============================================================================
# Farben / Präfixe für Konsolenausgabe
# ============================================================================
$Green = [char]27 + '[32m'
$Red = [char]27 + '[31m'
$Yellow = [char]27 + '[33m'
$Reset = [char]27 + '[0m'

function Write-Status {
    param([string] $Message, [string] $Status = "INFO")
    $timestamp = Get-Date -Format "HH:mm:ss"
    $prefix = switch ($Status) {
        "OK"   { "${Green}[OK]${Reset}" }
        "FAIL" { "${Red}[FAIL]${Reset}" }
        "WARN" { "${Yellow}[WARN]${Reset}" }
        default { "[INFO]" }
    }
    Write-Host "$prefix [$timestamp] $Message"
}

# ============================================================================
# Hilfsfunktion: CPU-%-Berechnung (korrekt!)
# ============================================================================
# Die rohe Process.CPU-Property ist nicht zuverlässig für point-in-time Messungen.
# Wir berechnen: (delta TotalProcessorTime) / (verstrichene Zeit) / (Kernanzahl)
#
# Logik:
#   1. Snapshot1: TotalProcessorTime, Wall-Clock-Zeit
#   2. Warte Intervall
#   3. Snapshot2: TotalProcessorTime, Wall-Clock-Zeit
#   4. CPU% = (TPT2 - TPT1) / (Wall2 - Wall1) / NumCores * 100
#
# Damit das funktioniert, speichern wir die letzte Messung und verwenden sie
# für die nächste Berechnung.

$script:lastCpuSnapshot = $null
$script:numCores = (Get-CimInstance -ClassName Win32_Processor |
                    Measure-Object -Property NumberOfLogicalProcessors -Sum).Sum

Write-Status "System-Kernanzahl: $($script:numCores)"

function Get-ProcessMetrics {
    <#
    .SYNOPSIS
        Holt Prozessmetriken (PID, Name, CPU%, Speicher, Handles, Threads).
    #>
    param([System.Diagnostics.Process] $Process)

    $now = Get-Date
    $tpt = $Process.TotalProcessorTime.TotalMilliseconds

    # CPU-Prozentsatz berechnen
    $cpu = 0.0
    if ($script:lastCpuSnapshot) {
        $timeDeltaSec = ($now - $script:lastCpuSnapshot.Timestamp).TotalSeconds
        if ($timeDeltaSec -gt 0) {
            $tptDeltaSec = ($tpt - $script:lastCpuSnapshot.TotalProcessorTime) / 1000
            $cpu = ($tptDeltaSec / $timeDeltaSec / $script:numCores) * 100
            # Null-Check und max 0 (sollte nicht vorkommen, aber defensive)
            if ($cpu -lt 0) { $cpu = 0 }
        }
    }

    # Aktuellen Snapshot für nächste Iteration speichern
    $script:lastCpuSnapshot = @{
        TotalProcessorTime = $tpt
        Timestamp = $now
    }

    # Speicher: WorkingSet = physikalischer RAM; PrivateMemory = alles (inkl. paged)
    $workingSetMB = [math]::Round($Process.WorkingSet64 / 1MB, 2)
    $privateMemoryMB = [math]::Round($Process.PrivateMemory64 / 1MB, 2)

    return @{
        Timestamp           = $now
        TimestampStr        = $now.ToString("yyyy-MM-dd HH:mm:ss")
        ProcessId           = $Process.Id
        ProcessName         = $Process.ProcessName
        CpuPercent          = [math]::Round($cpu, 2)
        WorkingSetMB        = $workingSetMB
        PrivateMemoryMB     = $privateMemoryMB
        HandleCount         = $Process.HandleCount
        ThreadCount         = $Process.Threads.Count
    }
}

# ============================================================================
# Main-Schleife
# ============================================================================

Write-Status "=== MerkWerk Langzeit-Ressourcentest Start ==="
Write-Status "Parameter:"
Write-Status "  Dauer: $DurationHours Stunden"
Write-Status "  Prozess: $ProcessName"
Write-Status "  Sampling-Intervall: $SampleIntervalSec Sekunden"
Write-Status "  CSV-Ausgabe: $OutCsv"
Write-Status ""

# Prozess finden
$process = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $process) {
    Write-Status "Prozess '$ProcessName' nicht gefunden. Bitte starten Sie den Daemon zuerst." FAIL
    exit 1
}

Write-Status "Prozess gefunden: PID=$($process.Id), Name=$($process.ProcessName)" OK
Write-Status "Test-Start: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Status ""

# Sampling-Liste initialisieren
$samples = @()
$testStartTime = Get-Date
$testEndTime = $testStartTime.AddHours($DurationHours)

# Initiale CPU-Messung (für Baseline)
$null = Get-ProcessMetrics -Process $process

Write-Status "Sampling beginnt..."

# Sampling-Schleife
while ((Get-Date) -lt $testEndTime) {
    # Prozess noch am Leben?
    $process = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
    if (-not $process) {
        Write-Status "Prozess '$ProcessName' ist nicht mehr vorhanden!" FAIL
        break
    }

    # Metriken holen und speichern
    $metrics = Get-ProcessMetrics -Process $process
    $samples += $metrics

    # Debug-Ausgabe (jede 10. Messung oder nach 1h, 4h, 8h)
    $elapsedHours = ((Get-Date) - $testStartTime).TotalHours
    if ($samples.Count % 10 -eq 0 -or ($elapsedHours -ge 1 -and $elapsedHours -le 1.05) -or
        ($elapsedHours -ge 4 -and $elapsedHours -le 4.05) -or ($elapsedHours -ge 8 -and $elapsedHours -le 8.05)) {
        Write-Status "[Sample $($samples.Count)] CPU=$($metrics.CpuPercent)% RAM=$($metrics.WorkingSetMB) MB | Verstrichen: $([math]::Round($elapsedHours, 2))h"
    }

    # Warte bis zur nächsten Messung
    Start-Sleep -Seconds $SampleIntervalSec
}

$testEndActual = Get-Date
$testDurationActual = ($testEndActual - $testStartTime).TotalSeconds

Write-Status ""
Write-Status "Sampling abgeschlossen. $($samples.Count) Messwerte in $([math]::Round($testDurationActual / 3600, 2)) Stunden."
Write-Status ""

# ============================================================================
# CSV exportieren
# ============================================================================

Write-Status "Exportiere zu CSV: $OutCsv"
$samples | Select-Object -Property TimestampStr, CpuPercent, WorkingSetMB, PrivateMemoryMB, HandleCount, ThreadCount |
    Export-Csv -Path $OutCsv -NoTypeInformation -Encoding UTF8

Write-Status "CSV erfolgreich geschrieben." OK

# ============================================================================
# Statistiken berechnen
# ============================================================================

Write-Status ""
Write-Status "=== Statistiken ==="

# CPU-Statistiken
$cpuValues = $samples.CpuPercent
$cpuAvg = $cpuValues | Measure-Object -Average | Select-Object -ExpandProperty Average
$cpuMax = $cpuValues | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum
$cpuMin = $cpuValues | Measure-Object -Minimum | Select-Object -ExpandProperty Minimum

Write-Host "CPU-Prozentsatz:"
Write-Host "  Durchschnitt: $([math]::Round($cpuAvg, 2))%"
Write-Host "  Maximum: $([math]::Round($cpuMax, 2))%"
Write-Host "  Minimum: $([math]::Round($cpuMin, 2))%"

# RAM-Statistiken (WorkingSet = physikalischer RAM)
$ramValues = $samples.WorkingSetMB
$ramAvg = $ramValues | Measure-Object -Average | Select-Object -ExpandProperty Average
$ramMax = $ramValues | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum
$ramMin = $ramValues | Measure-Object -Minimum | Select-Object -ExpandProperty Minimum

Write-Host ""
Write-Host "WorkingSet-RAM (physikalisch):"
Write-Host "  Durchschnitt: $([math]::Round($ramAvg, 2)) MB"
Write-Host "  Maximum: $([math]::Round($ramMax, 2)) MB"
Write-Host "  Minimum: $([math]::Round($ramMin, 2)) MB"

# Private Memory (alles, inkl. paged)
$privMemValues = $samples.PrivateMemoryMB
$privMemAvg = $privMemValues | Measure-Object -Average | Select-Object -ExpandProperty Average
$privMemMax = $privMemValues | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum

Write-Host ""
Write-Host "Private Memory (gesamt):"
Write-Host "  Durchschnitt: $([math]::Round($privMemAvg, 2)) MB"
Write-Host "  Maximum: $([math]::Round($privMemMax, 2)) MB"

# Handle- und Thread-Count
$handleValues = $samples.HandleCount
$handleAvg = $handleValues | Measure-Object -Average | Select-Object -ExpandProperty Average
$handleMax = $handleValues | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum

$threadValues = $samples.ThreadCount
$threadAvg = $threadValues | Measure-Object -Average | Select-Object -ExpandProperty Average
$threadMax = $threadValues | Measure-Object -Maximum | Select-Object -ExpandProperty Maximum

Write-Host ""
Write-Host "System-Handles:"
Write-Host "  Durchschnitt: $([math]::Round($handleAvg))"
Write-Host "  Maximum: $([math]::Round($handleMax))"

Write-Host ""
Write-Host "Threads:"
Write-Host "  Durchschnitt: $([math]::Round($threadAvg))"
Write-Host "  Maximum: $([math]::Round($threadMax))"

# ============================================================================
# Speicherleck-Heuristik (lineare Regression WorkingSet)
# ============================================================================

Write-Host ""
Write-Host "Speicherleck-Analyse:"

if ($samples.Count -gt 1) {
    # Einfache lineare Regression: WorkingSet vs. Sample-Index
    # Wenn die Steigung (Slope) deutlich positiv ist -> Leak-Verdacht

    $n = $samples.Count
    $x_vals = 0..($n - 1) # Sample-Indizes
    $y_vals = $ramValues

    # Berechne Mittelwerte
    $x_mean = ($x_vals | Measure-Object -Average).Average
    $y_mean = ($y_vals | Measure-Object -Average).Average

    # Berechne Steigung (Slope) und Intercept
    $numerator = 0
    $denominator = 0
    for ($i = 0; $i -lt $n; $i++) {
        $dx = $x_vals[$i] - $x_mean
        $dy = $y_vals[$i] - $y_mean
        $numerator += $dx * $dy
        $denominator += $dx * $dx
    }

    $slope = if ($denominator -gt 0) { $numerator / $denominator } else { 0 }

    # Steigung ist "Speicher pro Sample" (in MB/Sample)
    # Mit 30s Intervall und 8h = 960 Samples, extrapolieren wir:
    # Theoretischer Speicher-Zuwachs über 8h = slope * 960
    $projected_leak_mb = $slope * 960

    Write-Host "  Regressions-Steigung (WorkingSet): $([math]::Round($slope, 4)) MB/Sample"
    Write-Host "  Theoretischer 8h-Leak: $([math]::Round($projected_leak_mb, 2)) MB (bei linearem Trend)"

    # Heuristik: > 50 MB Anstieg über 8h ist verdächtig
    if ($projected_leak_mb -gt 50) {
        Write-Status "Mögliches Speicherleck erkannt (Projektion: +$([math]::Round($projected_leak_mb, 2)) MB über 8h)" WARN
    } else {
        Write-Status "Speicherverbrauch stabil (Projektion: +$([math]::Round($projected_leak_mb, 2)) MB über 8h)" OK
    }
} else {
    Write-Status "Zu wenige Samples für Regressions-Analyse" WARN
}

# ============================================================================
# PASS/FAIL-Bewertung
# ============================================================================

Write-Host ""
Write-Host "=== PASS/FAIL-Bewertung ==="

$pass = $true
$issues = @()

# Schwelle 1: CPU < 5% Durchschnitt
if ($cpuAvg -ge 5.0) {
    $issues += "CPU-Durchschnitt ($([math]::Round($cpuAvg, 2))%) >= 5% — nicht erfüllt"
    $pass = $false
} else {
    Write-Status "CPU-Durchschnitt: $([math]::Round($cpuAvg, 2))% < 5% ✓" OK
}

# Schwelle 2: RAM < 200 MB (Peak)
if ($ramMax -ge 200.0) {
    $issues += "WorkingSet-Peak ($([math]::Round($ramMax, 2)) MB) >= 200 MB — nicht erfüllt"
    $pass = $false
} else {
    Write-Status "WorkingSet-Peak: $([math]::Round($ramMax, 2)) MB < 200 MB ✓" OK
}

# Schwelle 3: Kein Leak (Regression-Check)
if ($samples.Count -gt 1 -and $projected_leak_mb -gt 50) {
    $issues += "Mögliches Speicherleck (Projektion: +$([math]::Round($projected_leak_mb, 2)) MB)"
    $pass = $false
} else {
    Write-Status "Speicherleck-Heuristik: OK ✓" OK
}

Write-Host ""
if ($pass) {
    Write-Status "=== ERGEBNIS: PASS ===" OK
    exit 0
} else {
    Write-Status "=== ERGEBNIS: FAIL ===" FAIL
    Write-Host ""
    Write-Host "Probleme:"
    foreach ($issue in $issues) {
        Write-Host "  - $issue"
    }
    exit 1
}
