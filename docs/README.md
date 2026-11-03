# The Shae Programming Language: Complete Architectural Guide & Book

This directory contains the source code and compiled PDF for **The Shae Programming Language: From Sand to Supercomputers** (The Complete Reference and Pedagogical Guide).

## File Overview

* **`shae_the_complete_guide.pdf`**: The compiled 42-page publication-ready PDF book.
* **`shae_the_complete_guide.tex`**: Master LaTeX document containing the styling, macro definitions, color schemes, and chapter inclusions.
* **`guide/`**: Modular chapter sources (Chapters 1 through 16).
    * `ch01_magic.tex`: The Magic of Computers and Why Shae Exists
    * `ch02_journey.tex`: The Journey of Code: From Human Text to Silicon Execution
    * `ch03_variables.tex`: The First Words: Variables, Memory Boxes, and Scope
    * `ch04_types.tex`: The Building Blocks: The Complete Type System
    * `ch05_control_flow.tex`: Making Decisions: Control Flow and Pattern Matching
    * `ch06_functions.tex`: Actions and Recipes: Functions, Closures, and Pipelines
    * `ch07_errors.tex`: When Things Go Wrong: Predictable Error Handling
    * `ch08_vm.tex`: Inside the Engine Room: Virtual Machine Architecture & Bytecodes
    * `ch09_concurrency.tex`: Concurrency: Fibers, Coroutines, and Channels
    * `ch10_actors.tex`: The Actor Model & Distributed Clustering (`std:actor` & `std:actor:cluster`)
    * `ch11_buffer.tex`: Raw Systems Programming & Zero-Copy Memory (`std:buffer`)
    * `ch12_llvm.tex`: Direct LLVM SSA Compilation & Native Machine Code (`shae llvm`)
    * `ch13_self_hosting.tex`: The Holy Grail: The Self-Hosting Compiler Pipeline
    * `ch14_tooling.tex`: The Developer Tooling Ecosystem
    * `ch15_distribution.tex`: Cross-Platform Distribution & The Universal Installer
    * `ch16_glossary.tex`: Complete Syntax Grammar & Vocabulary Glossary

## Rebuilding the PDF

To rebuild the PDF book:
```bash
cd docs
pdflatex -interaction=nonstopmode shae_the_complete_guide.tex
pdflatex -interaction=nonstopmode shae_the_complete_guide.tex
```
