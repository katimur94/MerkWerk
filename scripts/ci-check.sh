#!/bin/bash

# MerkWerk CI-Check — Platform-übergreifende Validierung (Linux-Sandbox + CI)
# Führt aus: Fmt-Check, native Tests, Windows-Crosscheck (mingw), App-Build (opt.)
# Bricht bei Fehler ab; Fmt-Diffs sind nur Warnungen.

set -euo pipefail

# Farb- und Statuspräfixe
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Zusammenfassung-Array
declare -a STEPS_RESULT

step_count=0
passed=0
skipped=0

# Hilfsfunktion: Step-Ergebnis loggen und speichern
log_step() {
    local status="$1"
    local desc="$2"
    ((step_count++))

    case "$status" in
        "OK")
            echo -e "${GREEN}[OK]${NC} $desc"
            STEPS_RESULT+=("${status}:${desc}")
            ((passed++))
            ;;
        "FAIL")
            echo -e "${RED}[FAIL]${NC} $desc"
            STEPS_RESULT+=("${status}:${desc}")
            ;;
        "SKIP")
            echo -e "${YELLOW}[SKIP]${NC} $desc"
            STEPS_RESULT+=("${status}:${desc}")
            ((skipped++))
            ;;
        "WARN")
            echo -e "${YELLOW}[WARN]${NC} $desc"
            STEPS_RESULT+=("${status}:${desc}")
            ((passed++))
            ;;
    esac
}

echo "=== MerkWerk CI-Check Start ==="
echo "Arbeitsverzeichnis: $(pwd)"
echo ""

# Schritt 1: Fmt-Check (nur Warnung)
echo "→ Schritt 1: cargo fmt --all --check (Daemon)"
if cd daemon && cargo fmt --all --check 2>&1; then
    log_step "OK" "Fmt-Check bestanden"
else
    log_step "WARN" "Fmt-Diffs gefunden (kein Blocker in Etappe 0)"
fi
cd - > /dev/null

echo ""

# Schritt 2: Native Tests
echo "→ Schritt 2: cargo test --workspace (native)"
if cd daemon && cargo test --workspace 2>&1; then
    log_step "OK" "Native Tests erfolgreich"
else
    log_step "FAIL" "Native Tests gescheitert"
    exit 1
fi
cd - > /dev/null

echo ""

# Schritt 3: Windows Clippy (Cross-Check)
echo "→ Schritt 3: cargo clippy --workspace --target x86_64-pc-windows-gnu (Lints)"
if cd daemon && cargo clippy --workspace --target x86_64-pc-windows-gnu -- -D warnings 2>&1; then
    log_step "OK" "Clippy (Windows) bestanden"
else
    log_step "FAIL" "Clippy (Windows) gescheitert"
    exit 1
fi
cd - > /dev/null

echo ""

# Schritt 4: Windows Check (Build-Validierung)
echo "→ Schritt 4: cargo check --workspace --target x86_64-pc-windows-gnu"
if cd daemon && cargo check --workspace --target x86_64-pc-windows-gnu 2>&1; then
    log_step "OK" "Windows-Check bestanden"
else
    log_step "FAIL" "Windows-Check gescheitert"
    exit 1
fi
cd - > /dev/null

echo ""

# Schritt 5: App-Build (optional)
echo "→ Schritt 5: npm run build (App, optional)"
if [ -d "app/node_modules" ] || command -v npm &> /dev/null; then
    if [ -d "app" ]; then
        if cd app && npm run build 2>&1; then
            log_step "OK" "App-Build erfolgreich"
        else
            log_step "WARN" "App-Build fehlgeschlagen (Etappe 0, nicht kritisch)"
        fi
        cd - > /dev/null
    else
        log_step "SKIP" "app/-Verzeichnis nicht gefunden"
    fi
else
    log_step "SKIP" "npm nicht vorhanden, App-Build übersprungen"
fi

echo ""
echo "=== Zusammenfassung ==="
for result in "${STEPS_RESULT[@]}"; do
    status="${result%%:*}"
    desc="${result#*:}"
    case "$status" in
        "OK")   echo -e "${GREEN}✓${NC} $desc" ;;
        "FAIL") echo -e "${RED}✗${NC} $desc" ;;
        "SKIP") echo -e "${YELLOW}○${NC} $desc" ;;
        "WARN") echo -e "${YELLOW}⚠${NC} $desc" ;;
    esac
done

echo ""
echo "Ergebnis: $passed/${step_count} Schritte erfolgreich ($skipped übersprungen)"
echo "=== MerkWerk CI-Check Ende ==="
