# Shae (Deutsch)

<div align="center">

```
   ____  _                 
  / ___|| |__   __ _  ___  
  \___ \| '_ \ / _` |/ _ \ 
   ___) | | | | (_| |  __/ 
  |____/|_| |_|\__,_|\___| 
```

**Eine moderne, strikte und ergonomische System-Scripting-Programmiersprache mit Cranelift JIT-Kompilierung.**  
*Keine schwere Konfiguration, nativer Maschinencode per JIT & OSR, RFC 6455 WebSockets, WebAssembly 2D-Grafik, Shae Studio Visual Debugger und zentraler Paketmanager.*

<p align="center">
  <a href="README.md">English</a> | <b>Deutsch</b>
</p>

[![Version](https://img.shields.io/badge/version-0.8.0-blue.svg)](Cargo.toml)
[![Tests](https://img.shields.io/badge/tests-380%2B%20passing-brightgreen.svg)]()
[![Rust](https://img.shields.io/badge/gebaut%20mit-Rust%202024-orange.svg)](https://www.rust-lang.org/)
[![Lizenz: MIT](https://img.shields.io/badge/Lizenz-MIT-blue.svg)](LICENSE)

</div>

---

## ✨ Was ist Shae?

Shae verbindet die Entwicklungsgeschwindigkeit dynamischer Sprachen mit der Ausführungsleistung und Zuverlässigkeit moderner kompilierter Systeme.

### Hauptmerkmale in Version 0.8.0:
1. **Tiered JIT & On-Stack Replacement (OSR)**:
   - Integrierte Cranelift-Code-Generierung für heiße Funktionen und Schleifen.
   - Nahtloser Zustandsübergang (OSR) während der Programmausführung direkt aus dem Bytecode-Interpreter in optimierten Maschinencode.
2. **\`std:ws\` RFC 6455 WebSocket-Engine**:
   - Reiner, synchron/asynchroner Vollduplex-WebSocket-Server und -Client ohne externe C-Abhängigkeiten.
   - RFC 6455- und RFC 7230-Konformität, Client-Maskierung, automatische Ping/Pong-Heartbeats und Resilienz gegen fehlerhafte Handshakes.
3. **WebAssembly 2D-Arcade-Runtime**:
   - Kompilierung von Shae-Grafik- und Spielecode direkt nach WebAssembly.
   - 60 FPS Software-Rasterung, Framebuffer-Austausch und Zero-Install-Player für moderne Webbrowser.
4. **Shae Studio & Visueller Debugger**:
   - Webbasierte Entwicklungsumgebung mit Monaco-Editor und DAP (Debug Adapter Protocol).
   - Haltepunkte, Einzelschritt-Steuerung (*Step In*, *Step Over*, *Step Out*), lokale Variableninspektion und Live-Framebuffer-Streaming.
5. **Zentraler Paketmanager (\`shae pkg\`)**:
   - Reproduzierbare Paketverwaltung mit SHA-256-Prüfsummenvalidierung und \`.shaepkg\`-Archiven (\`SHAEPKG1\`-Header).
   - Schnelle Paketinstallation: \`shae pkg add <paketname>\`.

---

## 🚀 Schnellstart

### Installation aus dem Quellcode:
```bash
git clone https://github.com/spdedsec/shae.git
cd shae
cargo build --release
```

### Ein erstes Programm schreiben (\`hallo.shae\`):
```shae
// Graduelle Typisierung mit Funktionsannotationen
fn begruessung(name: string) -> string {
    return "Hallo, " + name + "! Willkommen bei Shae v0.8.0."
}

print(begruessung("Welt"))
```

### Ausführen:
```bash
shae run hallo.shae
```

---

## 🧪 Test-Suite & Qualitätssicherung

Alle Testsuiten können über Cargo ausgeführt werden:
```bash
cargo test --lib
cargo test --test jit_test
cargo test --test ws_test
cargo test --test ide_test
cargo test --test e2e_tier1_features
cargo test --test e2e_tier2_boundaries
cargo test --test e2e_tier3_combinations
cargo test --test e2e_tier4_workloads
```

---

## 📜 Lizenz

Dieses Projekt ist unter der [MIT-Lizenz](LICENSE) lizenziert.
