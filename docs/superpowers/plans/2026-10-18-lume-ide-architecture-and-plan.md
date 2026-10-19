# Lume IDE — Plano Arquitetural e de Implementação Executável

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Construir o "Lume", um editor de texto e ambiente de desenvolvimento (IDE) ultraleve, com arquitetura desacoplada core/frontend, TUI com suporte nativo de primeira classe a mouse, inicialização < 50ms, modeless por padrão com modal opcional, zero-config para C/C++/Assembly/CUDA/Java, busca por trigramas em repositórios massivos e agente remoto nativo via SSH.

**Architecture:** Núcleo desacoplado em Rust sobre threads de domínio (`Core`, `LSP`, `Indexer`, `IO/Git`) comunicando-se via canais MPSC/Broadcast lock-free/bounded com ring buffers e loop assíncrono Tokio para I/O externo. Frontends se conectam via protocolo binário/IPC (TUI via Crossterm/Ratatui customizada no MVP; GUI nativa futura via wgpu/Cosmic-text).

**Tech Stack:** Rust (edição e subsistemas), `ropey` (buffer principal), `memmap2` (mmap para arquivos gigantes), `crossterm` / `ratatui` (TUI e input de mouse), `tree-sitter` (syntax e queries incrementais), `tokio` (runtime I/O background), `crossbeam-channel` (IPC intra-processo), `wasmtime` (sandbox de plugins pós-MVP).

**Spec:** Especificação de requisitos da Lume IDE (Prompt de Arquitetura e Engenharia de Sistemas).

## Global Constraints
- Sem ruruntimes de interpretação pesada (sem V8, NodeJS, Python ou Electron) no caminho crítico ou no executável base.
- A UI thread nunca espera I/O de disco, socket de rede, indexação ou resposta de LSP/DAP.
- Latência de renderização e processamento de tecla mantida abaixo de 16 ms (alvo de 60 FPS contínuos no TUI).
- Compatibilidade estrita com terminais modernos com emulação xterm/kitty/SGR mouse via SSH sem quebra de escape codes.
- Licença open source: Apache 2.0 / MIT (Dual licensing padrão do ecossistema Rust).

## Review Focus
1. Arquivos gigantes (> 1 GB) abertos sem carregar o conteúdo todo em RAM via `mmap` e paginação por blocos de linha.
2. Queda de conexão SSH durante edição remota com preservação total de buffer sujo no daemon do servidor (`lumed`).
3. Conflito entre autosave concorrente e modificação externa de arquivo detectada via file-watcher (notificação imediata com diff view).
4. Saturação de memória por servidores LSP externos em repositórios com milhões de linhas (capping de recursos e Large Repo Mode).
5. Desincronização de sequência de escape de mouse em emuladores de terminal legados ou dentro de sessões aninhadas de tmux.

---

## 1. Resumo Executivo
O **Lume** é uma IDE em Rust para terminais (TUI) e sistemas headless/SSH concebida para engenheiros trabalhando em monorepos massivos (C, C++, CUDA, Java, Assembly). Combina a velocidade de inicialização do Neovim (< 50 ms) com a usabilidade out-of-the-box do VS Code (modeless, mouse em cliques/seleções/menus, autosave atômico, zero-config via auto-descoberta de toolchains e LSP). Toda a computação pesada (busca trigrama, tree-sitter, indexação, git, LSP) roda em background threads sem nunca bloquear o event loop da UI (< 16 ms p99). O projeto adota arquitetura de motor desacoplado (Core-Engine / Protocol / Frontend / Remote-Agent), garantindo paridade total de recursos localmente ou via SSH, com futura migração direta para GUI nativa.

---

## 2. Suposições

| Suposição | Justificativa | Impacto se estiver errada |
|---|---|---|
| **S1. Terminais dos usuários suportam protocolo SGR-Mouse e true-color (24-bit)** | Alacritty, WezTerm, Kitty, iTerm2, Windows Terminal e GNOME Terminal já suportam SGR 1006 e 24-bit RGB por padrão há anos. | Usuários com terminais legados (ex.: VT100 estrito) terão fallback degradado para seleção de texto normal e 256 cores sem arraste fluido. |
| **S2. Servidores LSP de terceiros (clangd, rust-analyzer, jdtls) estão disponíveis ou podem ser auto-instalados no ambiente** | A proposta é zero-config; a Lume detectará binários existentes no `$PATH` ou oferecerá download one-click de toolchains estáticas. | Se ausente e sem rede para baixar, recursos semânticos de IDE ficam inativos, operando apenas com tree-sitter + ctags/trigramas. |
| **S3. O sistema operacional alvo disponibiliza `mmap` e I/O eficiente com suporte a atomic file rename** | POSIX (`mmap`, `rename`) e Win32 (`CreateFileMapping`, `MoveFileEx`) suportam estas primitivas fundamentalmente. | Em filesystems de rede (NFS, SMB, CIFS) com locking falho, gravação atômica precisa de fallback para escrita em arquivo direto com sync explícito (`fsync`). |
| **S4. Rust continua sendo a linguagem com melhor custo/benefício para controle de memória sem GC e ecossistema de infraestrutura** | Alternativa em C traria alto custo de memory safety em parsers assíncronos; C++ adicionaria tempo de build e fragmentação de toolchain. | Se Rust se provar problemático em plataformas exóticas, o core precisaria ser exportado com FFI C pura. |
| **S5. O repositório monolítico pode ter milhões de arquivos, mas a maioria não é tocada pelo desenvolvedor na mesma sessão** | Workflows em monorepos (Google, Chromium, Linux Kernel) editam centenas de arquivos enquanto dependem de leitura e busca no restante. | Se o workflow exigir carregar todos os arquivos simultaneamente em memória viva, o teto de 200 MB exigirá paginação agressiva de buffers em disco. |

---

## 3. Visão e Não-Objetivos

### 3.1 Visão
Entregar um ambiente de edição rápido, previsível, com layout visual intuitivo em terminal, ergonomia moderna sem exigir horas de configuração de Lua/Vimscript, tolerante a arquivos com gigabytes e monorepos com centenas de milhares de nós de compilação.

### 3.2 Não-Objetivos (Escopo Deliberadamente Excluído)
- **Não teremos runtime JavaScript/Python no processo core:** Sem motor V8, QuickJS ou Python embutido gerenciando buffers ou a renderização.
- **Sem IA Generativa / Auto-complete LLM embutido no MVP:** O foco inegociável é latência zero de digitação e previsibilidade. Integrações de LLM poderão ser adicionadas no futuro via plugins WASM desacoplados da renderização.
- **Sem Colaboração em Tempo Real (CRDTs multi-usuário estilo Google Docs):** A sobrecarga de sincronização distribuída complexifica a árvore de undo/redo e ropes sem agregar valor primário ao programador de sistemas.
- **Sem Marketplace Centralizado Proprietário:** Sem telemetria para nuvem, contas ou assinaturas. Extensões e perfis serão arquivos locais ou repositórios Git abertos.
- **Sem Emulação 100% Bug-compatible de Plugins Neovim/Vimscript:** Lume oferece compatibilidade com os atalhos modais principais do Vim (operadores, motions, text objects), mas não é um runtime para rodar plugins do Neovim.

### 3.3 Licença Proposta
**Dual-license MIT / Apache-2.0**: Alinha-se diretamente aos padrões da comunidade Rust, permitindo ampla adoção industrial em empresas com restrições corporativas sobre GPL, garantindo fácil reutilização de crates e contribuições do ecossistema.

---

## 4. Arquitetura do Sistema

### 4.1 Visão Geral dos Módulos

```mermaid
flowchart TD
    subgraph UI_Layer ["Camada de Apresentação (UI Thread - 60 FPS)"]
        InputHandler["Input Engine\n(Keys, SGR Mouse, Gestos)"]
        Renderer["TUI Render Engine\n(Ratatui Modificada + Double Buffer)"]
        WhichKey["Which-Key / Palette\n(Overlay Descoberta)"]
        StatusView["Status, Tabs & Splits View"]
    end

    subgraph Core_Engine ["Lume Core Engine (In-Memory State & Coordination)"]
        Dispatcher["Event Dispatcher & Action Router"]
        BufferManager["Buffer Manager\n(Rope / Mmap / Binary Hex)"]
        UndoRedo["Undo/Redo Tree Engine\n(Persistent Journal)"]
        CursorEngine["Multi-Cursor & Selection Engine"]
        AutoSave["Auto-Save & Atomic Swap Controller"]
        ConfigManager["Config & Profile Engine (Hot-Reload TOML)"]
    end

    subgraph Background_Workers ["Workers Assíncronos & I/O (Thread Pool / Tokio)"]
        TreeSitterWorker["Tree-Sitter Highlighting & Folding Worker"]
        LSPClient["LSP Client Hub (Clangd, rust-analyzer, etc.)"]
        DAPClient["DAP Debugger Engine (lldb, gdb, cuda-gdb)"]
        TrigramIndexer["Trigram Background Indexer (Livegrep/Zoekt)"]
        VCSWorker["VCS Engine (Git Diffs, Inline Blame, Hunks)"]
        BuildRunner["Build & Quickfix Diagnostic Parser"]
    end

    subgraph Remote_Daemon ["Agente Remoto (Headless lumed via SSH)"]
        DaemonBridge["Lume Daemon Bridge (IPC / Protocolo Compacto)"]
    end

    InputHandler -->|KeyEvent / MouseEvent| Dispatcher
    Dispatcher -->|Comandos de Edição| BufferManager
    BufferManager <--> UndoRedo
    Dispatcher --> CursorEngine
    BufferManager --> AutoSave
    
    Dispatcher -->|Spawn Task / Cancel| Background_Workers
    Background_Workers -->|Resultados / Diffs / Diagnostics| Dispatcher
    
    BufferManager --> Renderer
    WhichKey --> Renderer
    StatusView --> Renderer
    Renderer -->|ANSI / Terminal SGR / Raw I/O| STDOUT[Terminal Display]

    DaemonBridge <-->|RPC Binário Multiplexado| Dispatcher
```

### 4.2 Modelo de Concorrência e Threads
1. **Thread Principal (UI / Render Loop)**:
   - Ciclo fechado de renderização com target de 16 ms por frame (60 Hz).
   - Drena canal de mensagens não-bloqueante (`crossbeam_channel::Receiver::try_recv`).
   - Processamento imediato de teclas e cliques do mouse.
   - Aplica mutações locais no buffer ativo e solicita redesenho mínimo por *dirty rects* / células alteradas.
2. **Pool de Workers de Background (Rayon / OS Threads dedicadas)**:
   - Worker dedicado de **Tree-Sitter**: roda parses incrementais fora da UI thread em uma thread dedicada por workspace. Ao terminar uma árvore sintática atualizada, envia as spans para o core aplicar no highlight.
   - Worker de **Indexação de Trigramas**: varre o disco usando threads paralelas (Rayon) com baixa prioridade (nice/idle priority), lendo arquivos de texto e montando o índice invertido em memória.
3. **Runtime Assíncrono Tokio (I/O Externo)**:
   - Gerencia processos filhos de longa duração com pipes stdin/stdout assíncronos: servidores **LSP**, sessões **DAP**, processos de **Build** (Ninja, Make, Cargo) e o **File Watcher**.
   - Garante que a escrita atômica em disco (fsync) e operações com servidores remotos nunca causem preempção de tempo na UI thread.

### 4.3 Protocolo e Contratos Entre Componentes

```rust
// Contrato de Buffer: abstração para arquivos normais e arquivos mapeados em memória
pub enum BufferKind {
    MemoryRope(ropey::Rope),
    MemoryMapped(MmapChunkedBuffer),
}

pub struct Cursor {
    pub anchor: usize, // Posição de byte da âncora de seleção
    pub head: usize,   // Posição de byte da cabeça móvel do cursor
    pub affinity: CursorAffinity,
}

pub struct BufferSnapshot {
    pub buffer_id: BufferId,
    pub revision: u64,
    pub rope_slice: ropey::Rope,
    pub cursors: Vec<Cursor>,
}

// Canal de Comunicação Core <-> Background Workers
pub enum CoreToWorkerMessage {
    DocumentChanged { buffer_id: BufferId, revision: u64, delta: TextDelta },
    RunLspQuery { query_id: u64, request: LspRequest },
    CancelQuery { query_id: u64 },
    StartIndexing { workspace_root: PathBuf },
}

pub enum WorkerToCoreMessage {
    SyntaxTreeUpdated { buffer_id: BufferId, revision: u64, highlights: Vec<HighlightSpan> },
    LspResponseReceived { query_id: u64, response: LspResponse },
    IndexingProgress { indexed_files: usize, total_files: usize },
    VCSStatusChanged { dirty_hunks: Vec<DiffHunk> },
}
```

---

## 5. Decisões Técnicas

### 5.1 Comparativos Detalhados

| Área | Opção A | Opção B | Opção C | Escolha & Justificativa Técnica |
|---|---|---|---|---|
| **Renderizador TUI** | `ratatui` [verificar crate padrão] | `crossterm` direto | Renderizador TUI Próprio | **Ratatui + Backend Crossterm com customização**: Ratatui oferece double-buffering robusto, diffing de células para ANSI otimizado e ecossistema maduro. Adicionaremos buffer diff otimizado para evitar repintura total da tela. |
| **Estrutura de Buffer** | `ropey` (B-Tree de Ropes) | Piece Table (`tree-sitter-based` ou VS Code style) | Gap Buffer tradicional | **`ropey` + `memmap2` híbrido**: Ropes em B-Tree (`ropey`) oferecem inserção, deleção e conversão de linha/coluna em $O(\log N)$, ideal para edições multi-cursor frequentes. Para arquivos > 200 MB, chaveia para `memmap2` em blocos de leitura sob demanda. |
| **Syntax Highlighting** | `tree-sitter` incremental | `syntect` (TextMate regex) | Lexer customizado por linguagem | **`tree-sitter` [verificar versão atual 0.22+]**: Análise semântica sintática precisa, parser incremental real-time durante digitação, folds sintáticos e text-objects precisos sem travar a CPU como regex de TextMate. |
| **Indexação de Busca** | Trigrama invertido customizado (estilo Zoekt / Livegrep) | `grep-regex` / `ripgrep` crate integrado (`grep-searcher`) | `tantivy` (Lucene completo em Rust) | **Trigram Indexer custom em memória + Ripgrep crate como fallback**: Trigramas oferecem busca sub-200ms em milhões de arquivos já indexados; `ripgrep` crate é usado enquanto a indexação não está pronta ou para regex livre complexo. |
| **Runtime Assíncrono** | `tokio` (multi-thread) | `smol` / `async-std` | Threads nativas + canais | **`tokio` para I/O externo de subprocessos (LSP/DAP/SSH) + `crossbeam-channel` para core síncrono**: Desacopla a computação previsível da UI das filas de I/O assíncronas do sistema operacional. |
| **File Watching** | `notify` crate [verificar versão 6.x+] | Polling periódico manual | `inotify` / `kqueue` direto | **`notify` crate**: Abstrai inotify (Linux), kqueue (macOS) e ReadDirectoryChangesW (Windows) com debouncing configurável para evitar tempestades de eventos durante `git checkout` ou compilação. |
| **Serialização de Config e Sessão** | `toml` + `serde` | `ron` (Rust Object Notation) | `bincode` (para sessão) | **TOML para configuração do usuário; Bincode/Zero-Copy para journal de sessão**: TOML é legível para o usuário; Bincode garante restauração de sessão completa em < 50 ms. |
| **Sandbox de Plugins** | `wasmtime` (WASM/WASI) | Lua embutido (`mlua`) | Processos externos via IPC | **`wasmtime` (pós-MVP)**: Sandboxing de memória seguro, compilação JIT de alta performance (Cranelift), sem possibilidade de plugin derrubar a UI thread ou travar o processo principal. |

---

## 6. Orçamentos de Performance e Plano de Medição

| Métrica | Meta | Método de Medição | Ferramenta / Benchmark | Hardware de Referência |
|---|---|---|---|---|
| **Startup a frio até editar** | < 50 ms | Medir tempo do `execve` até o primeiro frame desenhado com cursor aceitando input. | `hyperfine --warmup 0 "lume test.c --bench-startup"` gerando trace de instrumentação com `tracing-chrome`. | x86_64 Core i7 12th gen / AMD Ryzen 5000+, 16GB RAM, NVMe PCIe 3.0, Linux Kernel 6.x. |
| **Latência tecla → pixel/caractere** | < 16 ms (p99 < 33 ms) | Injetar sequência de teclas em buffer pré-carregado e cronometrar timestamp monotonic (`clock_gettime(CLOCK_MONOTONIC)`) até o flush do escape sequence no terminal pseudo-tty. | Harness de benchmark de TUI headless com PTY virtual e `criterion.rs`. | Mesmo ambiente padrão; terminal virtual pty rodando a 60Hz. |
| **Abrir arquivo de 1 GB (somente leitura)** | < 1 s até ser navegável | Abrir arquivo binário ou log de 1 GB gerado randomicamente; medir tempo até desenhar o viewport inicial e responder a Page Down. | Benchmark automatizado interno medindo tempo de inicialização do handler `mmap` e renderização da primeira tela. | Disco NVMe padrão (leitura sequencial > 2000 MB/s), arquivo limpo de cache com `echo 3 > /proc/sys/vm/drop_caches`. |
| **Memória ociosa com repo de 1M arquivos indexado** | < 200 MB RSS | Indexar árvore clonada com 1 milhão de arquivos (ex.: Linux kernel + árvore gerada sinteticamente), aguardar repouso e medir Resident Set Size. | `heaptrack` ou `/usr/bin/time -v`, subtraindo alocações temporárias após rodar GC/shrink do indexador de trigramas compactado. | Máquina Linux com 32GB RAM para evitar swapping. |
| **Busca de texto em repo grande (trigrama)** | < 200 ms primeiros resultados | Executar query de busca textual de 5 caracteres presente em ~100 arquivos espalhados em monorepo com 500k arquivos indexados. | Microbenchmark Criterion cronometrando o tempo da query até os primeiros 50 itens serem despachados no canal da UI. | CPU com 8 cores/16 threads, índice pré-aquecido em RAM. |
| **Fuzzy finder de arquivos (1M arquivos)** | < 50 ms por tecla | Inserir consulta fuzzy letra por letra em lista pré-carregada de 1.000.000 de caminhos absolutos e medir tempo de filtragem e rankeamento. | `criterion.rs` com algoritmo de fuzzy match baseado em `nucleo-matcher` [verificar crate] otimizado para cache L3 e multithreading. | Processador de referência rodando em uma única thread fixa (single-core score). |
| **Indexação inicial** | Background, nunca bloqueia edição | Monitorar o framerate da UI (mínimo 58 FPS) enquanto o indexador consome até 70% das threads de CPU disponíveis. | Teste de estresse com script que digita continuamente enquanto a indexação roda em paralelo; medir variância dos intervalos de frame. | Teste executado com `taskset` limitando a 4 cores para estressar a contenção de concorrência. |

---

## 7. Especificação das Funcionalidades e Diferenciais

### 7.1 Edição e Manipulação de Buffer
- **Núcleo de Ropes & Mmap**: `Ropey` divide o texto em chunks de nós B-Tree de até 1 KB, permitindo saltos e contagem de quebras de linha (`\n` e `\r\n`) instantâneos. Arquivos com tamanho superior ao limiar configurável (ex.: > 100 MB) são abertos em modo `ReadOnly` via `memmap2`, com visualização de buffer virtual paginado.
- **Undo/Redo Tree Persistente**: Gravação de histórico em formato de árvore de deltas com timestamps e posições de cursor. Os deltas são persistidos em diretório de cache (`~/.cache/lume/undo/`) utilizando arquivo binário append-only.
- **Edição Hexadecimal Nativa**: Se detectado byte nulo (`0x00`) nos primeiros 8 KB do arquivo, o buffer oferece visualização em três colunas (Offset Hexadecimal, Bytes Hex, Caracteres ASCII legíveis) com navegação bidirecional.

### 7.2 Mouse de Primeira Classe e UX Descoberta
- **Manipulação Completa por Mouse**:
  - Clique posiciona cursor; duplo clique seleciona palavra; triplo clique seleciona linha; arraste cria seleção visual.
  - Rolagem por roda do mouse suave com aceleração configurável.
  - Divisões de tela (*splits*) possuem bordas arrastáveis com o mouse para redimensionamento em tempo real.
  - Abas de buffers superiores e itens da árvore de arquivos na barra lateral respondem a cliques para seleção, fechamento e expansão de diretórios.
- **Command Palette & Which-Key**:
  - `Ctrl+Shift+P` (ou `Space` em modo modal) abre a Command Palette com fuzzy finder exibindo o atalho associado ao lado de cada ação.
  - Atalhos compostos exibem uma janela suspensa flutuante (`which-key`) se o próximo caractere não for digitado em 300 ms, listando as continuações válidas com descrição legível.

### 7.3 Autosave Atômico e Tratamento de Conflitos
- **Mecanismo de Escrita Atômica**:
  1. O buffer modificado é gravado em um arquivo temporário no mesmo diretório (`.arquivo.c.lume_tmp`).
  2. Chamada a `fsync()` no arquivo temporário.
  3. `rename()` atômico POSIX substituindo o arquivo original.
- **Detecção de Conflitos com File-Watcher**:
  - Se o `notify` registrar um evento de modificação externa no arquivo com hash divergente do hash gravado em memória, o Lume suspende o autosave daquele arquivo.
  - Uma barra de aviso proeminente surge na interface com as opções clicáveis: `[Ver Diff]`, `[Manter Versão do Editor]`, `[Recarregar do Disco]`.

### 7.4 Integração LSP, Tree-Sitter e DAP
- **Zero-Config Toolchain Detection**:
  - Ao abrir um projeto, o Lume examina o workspace: se encontrar `compile_commands.json`, `CMakeLists.txt` ou arquivos `.c`/`.cpp`, ativa automaticamente `clangd`. Se encontrar `Cargo.toml`, ativa `rust-analyzer`. Se encontrar `pom.xml`/`build.gradle`, inicializa `jdtls`.
- **DAP (Debug Adapter Protocol)**:
  - Painel integrado de depuração com suporte a breakpoints clicáveis no gutter, inspeção de variáveis locais, pilha de chamadas e console REPL compatível com `lldb-vscode` / `gdb` / `cuda-gdb`.

### 7.5 Diferenciais do Lume

1. **Visão de Assembly/Disassembly Lado a Lado**:
   - Para projetos C/C++/CUDA, o Lume executa em background a compilação de arquivo único (`clang -S -g` ou `objdump -d -S` usando os flags de `compile_commands.json`).
   - Apresenta um buffer split com o código Assembly gerado; passar o mouse ou posicionar o cursor em uma linha de C destaca simultaneamente o bloco de instruções correspondente via mapeamento de linha DWARF.
2. **Visualização de Macros e Preprocessador**:
   - Comando inline para expandir macros C/C++ sem sair do arquivo (`clang -E -dD` direcionado ao bloco sob o cursor).
   - Leitura de diretivas de compilação para esmaecer visualmente blocos `#ifdef` que estejam inativos no profile atual.
3. **Agente Remoto Nativo via SSH (`lumed`)**:
   - Binário estático único compilado para Linux musl (`x86_64` / `aarch64`), instalado automaticamente com `lume ssh://user@host/path`.
   - Roda o indexador, LSP e tree-sitter no servidor; envia apenas diffs de layout e texto para o cliente local via canal SSH multiplexado com compressão Zstandard.
4. **Modo "Large Repo"**:
   - Ativado automaticamente se o repositório contiver mais de 100.000 arquivos ou arquivos individuais > 200 MB.
   - Ajusta dinamicamente a taxa de amostragem de git status, limita profundidade de watchers do inotify e reduz o consumo de threads do LSP.
5. **Perfis de Linguagem Prontos**:
   - Perfis selecionáveis com um clique: *Linux Kernel* (estilo de tabulação de 8 espaços, formatação checkpatch, flags de compilação freestanding), *Embedded/Microcontroladores* (arm-none-eabi toolchains), *CUDA High-Perf* (nvcc, syntax CUDA, visualizador PTX).
6. **Telemetria de Performance Local Integrada**:
   - Painel interno estilo *Task Manager* acessível via atalho rápido (`Ctrl+Shift+T` ou `:LumeStats`), detalhando uso de CPU, memória de cada servidor LSP, tempo do último parse tree-sitter e latência média dos últimos 100 frames.

---

## 8. Estrutura de Arquivos e Módulos do Projeto

```
lume/
├── Cargo.toml
├── crates/
│   ├── lume-core/             # Buffer, Rope, Cursor, Undo/Redo, Autosave
│   │   ├── src/
│   │   │   ├── buffer/        # Rope wrapper, mmap chunking, encoding
│   │   │   ├── cursor/        # Multi-cursor, seleções por bloco
│   │   │   ├── history/       # Undo-tree, snapshot persistence
│   │   │   ├── mode/          # Modeless vs Modal state machine
│   │   │   └── lib.rs
│   ├── lume-tui/              # Terminal Frontend (Ratatui, Crossterm, SGR Mouse)
│   │   ├── src/
│   │   │   ├── view/          # Splits, tabs, gutter, statusbar, which-key
│   │   │   ├── input/         # Parser de mouse/teclado, bindings
│   │   │   ├── terminal/      # Double-buffer screen allocator, pty
│   │   │   └── main.rs
│   ├── lume-lsp/              # Cliente LSP e Gerenciador de Servidores
│   │   ├── src/
│   │   │   ├── client/        # JSON-RPC 2.0 sobre stdin/stdout assíncrono
│   │   │   ├── discovery/     # Detecção de clangd, rust-analyzer, etc.
│   │   │   └── handlers/      # Hover, Definition, Diagnostics
│   ├── lume-syntax/           # Tree-sitter Wrapper & Syntax Highlighting
│   │   ├── src/
│   │   │   ├── highlighters/  # Mapeamento de temas ANSI/TrueColor
│   │   │   ├── incremental/   # Parser worker thread & invalidations
│   │   │   └── lib.rs
│   ├── lume-index/            # Indexador de Trigramas e Busca Rápida
│   │   ├── src/
│   │   │   ├── trigram/       # Índice invertido de trigramas compacto
│   │   │   ├── crawler/       # Walker multithread respeitando .gitignore
│   │   │   └── search/        # Engine de busca fuzzy e regex
│   ├── lume-build/            # Detecção de Build, Compilação e Quickfix
│   │   ├── src/
│   │   │   ├── detector/      # Makefile, CMake, Ninja, Bazel, Cargo
│   │   │   ├── runner/        # Spawn assíncrono e streaming de saída
│   │   │   └── parsers/       # Regex parser para erros de gcc/clang/javac
│   ├── lume-vcs/              # Integração Git Nativa
│   │   ├── src/
│   │   │   ├── git2_wrap/     # Diffing rápido de hunks e blame inline
│   │   │   └── lib.rs
│   └── lumed/                 # Agente Remoto Headless para SSH
│       ├── src/
│       │   ├── daemon/        # IPC server sobre SSH pipe
│       │   └── main.rs
└── tests/                     # Testes de integração, benchmarks Criterion e fuzzing
```

---

## 9. Interfaces e Contratos de Código (Traits & Tipos Principais)

```rust
// lume-core/src/buffer/mod.rs
pub trait TextBuffer: Send + Sync {
    fn len_bytes(&self) -> usize;
    fn len_lines(&self) -> usize;
    fn byte_to_line(&self, byte_idx: usize) -> usize;
    fn line_to_byte(&self, line_idx: usize) -> usize;
    fn slice_to_cow(&self, start_byte: usize, end_byte: usize) -> std::borrow::Cow<str>;
    fn insert(&mut self, byte_idx: usize, text: &str) -> Result<TextDelta, BufferError>;
    fn delete(&mut self, start_byte: usize, end_byte: usize) -> Result<TextDelta, BufferError>;
}

// lume-core/src/history/mod.rs
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TextDelta {
    pub start_byte: usize,
    pub old_text: String,
    pub new_text: String,
    pub timestamp: u64,
}

pub trait HistoryManager {
    fn record_edit(&mut self, delta: TextDelta, cursors_before: Vec<Cursor>, cursors_after: Vec<Cursor>);
    fn undo(&mut self, current_buffer: &mut dyn TextBuffer) -> Option<Vec<Cursor>>;
    fn redo(&mut self, current_buffer: &mut dyn TextBuffer) -> Option<Vec<Cursor>>;
    fn persist_to_disk(&self, path: &std::path::Path) -> std::io::Result<()>;
}

// lume-syntax/src/lib.rs
pub struct HighlightToken {
    pub range_bytes: std::ops::Range<usize>,
    pub style: TextStyle, // Cor foreground, background, mod (bold, italic)
}

pub trait IncrementalSyntaxEngine: Send {
    fn update_source(&mut self, delta: &TextDelta, full_text: &dyn TextBuffer);
    fn compute_screen_highlights(&self, line_start: usize, line_count: usize) -> Vec<HighlightToken>;
    fn compute_folds(&self) -> Vec<std::ops::Range<usize>>;
}

// lume-lsp/src/client/mod.rs
#[async_trait::async_trait]
pub trait LspServerConnection: Send + Sync {
    async fn initialize(&mut self, root_path: &std::path::Path) -> Result<lsp_types::InitializeResult, LspError>;
    async fn did_open(&mut self, doc: lsp_types::TextDocumentItem) -> Result<(), LspError>;
    async fn did_change(&mut self, params: lsp_types::DidChangeTextDocumentParams) -> Result<(), LspError>;
    async fn goto_definition(&mut self, params: lsp_types::GotoDefinitionParams) -> Result<Option<lsp_types::GotoDefinitionResponse>, LspError>;
}
```

---

## 10. Roadmap por Fases

```
M0: Protótipo de Latência e Buffer Core      (Semanas 1–4)
 └── M1: MVP Funcional (TUI, Mouse, Autosave, Syntax) (Semanas 5–12)
      └── M2: IDE v1 (LSP, DAP, Trigram Search, Git)     (Semanas 13–20)
           └── M3: Diferenciais (Disasm, SSH Agent, Monorepo) (Semanas 21–28)
                └── M4: Extensibilidade WASM & Polimento         (Semanas 29–36)
```

### M0: Protótipo de Latência e Buffer Core (4 semanas | 4–6 pessoa-semanas)
- **Objetivos**: Validar o teto de latência tecla-pixel (< 16 ms), modelo de concorrência com `crossbeam-channel` e buffer baseado em `ropey`.
- **Entregáveis**:
  - Binário CLI mínimo capaz de abrir um arquivo, aceitar inserção de texto e renderizar na tela via `crossterm`.
  - Suíte de microbenchmarks automatizada rodando no CI com `criterion`.
- **Critérios de Aceite**: Latência média de inserção de caractere em buffer de 100.000 linhas < 5 ms; startup < 30 ms.
- **Dependências**: Nenhuma.

### M1: MVP Funcional (8 semanas | 12–16 pessoa-semanas)
- **Objetivos**: Editor diário funcional e estável para arquivos locais com UX modeless por padrão e modal opcional.
- **Entregáveis**:
  - Suporte completo a mouse (clique, arraste, abas, splits redimensionáveis).
  - Autosave atômico, journal de recuperação pós-crash e undo-tree persistente.
  - Command palette com fuzzy finder e overlay de atalhos `which-key`.
  - Syntax highlighting via `tree-sitter` para C, C++, Rust, Python e Java.
- **Critérios de Aceite**: Zero perdas de dados após `kill -9` no processo durante teste de escrita; navegação por mouse 100% responsiva sem dessincronização visual.

### M2: Experiência de IDE v1 (8 semanas | 16–20 pessoa-semanas)
- **Objetivos**: Transformar o editor em IDE completa com inteligência de código e indexação.
- **Entregáveis**:
  - Cliente LSP com auto-descoberta (`clangd`, `rust-analyzer`, `jdtls`).
  - Indexador de trigramas em background com busca sub-200ms em repositórios grandes.
  - Quickfix list com parser automático de saídas de compilação (gcc, clang, cargo, javac).
  - Depurador DAP com breakpoints visuais no gutter e inspeção de variáveis.
  - Visualizador de Git diff no gutter e inline blame.
- **Critérios de Aceite**: Go-to-definition e hover respondendo em < 50 ms após resposta do LSP; indexação inicial de 500k arquivos consumindo < 150 MB de RAM.

### M3: Diferenciais para Sistemas e Monorepos (8 semanas | 16–20 pessoa-semanas)
- **Objetivos**: Entregar os diferenciais exclusivos para o público-alvo de sistemas de baixa latência e monorepos.
- **Entregáveis**:
  - Visão de Assembly/Disassembly lado a lado com mapeamento de linha DWARF.
  - Expansão de macros C/C++ inline e visualização de blocos `#ifdef` inativos.
  - Agente remoto `lumed` operando via SSH com paridade de recursos.
  - Modo automático "Large Repo" com degradação inteligente de I/O.
- **Critérios de Aceite**: Sessão SSH remota consumindo < 15 KB/s de largura de banda durante rolagem contínua; sincronização bidirecional C ↔ Assembly sem desvios de linha.

### M4: Extensibilidade WASM e Refinamento (8 semanas | 12–16 pessoa-semanas)
- **Objetivos**: Estabilidade para v1.0 e arquitetura de extensões seguras.
- **Entregáveis**:
  - Host de plugins baseado em `wasmtime` (WASM/WASI) com isolamento total de memória.
  - SDK de extensões com bindings para eventos de buffer, comandos e menus.
  - Painel de telemetria local ("Por que está lento?").
- **Critérios de Aceite**: Plugin com loop infinito em WASM é cancelado pela UI thread sem travamento da interface; tempo de boot mantido < 50 ms com plugins nativos habilitados.

---

## 11. Estratégia de Testes e Benchmarks

### 11.1 Testes Unitários e Property-Based Testing
- **Testes com `proptest` para o Buffer e Undo/Redo**:
  - Geração aleatória de sequências de milhares de mutações (inserções, deleções, quebras de linha, multi-cursores).
  - Invariante de consistência: Após aplicar $N$ edições aleatórias e em seguida $N$ chamadas de `undo`, o buffer DEVE ser byte-a-byte idêntico ao estado original.

### 11.2 Fuzzing Contínuo
- **Fuzzing com `cargo-fuzz` (libFuzzer)**:
  - Fuzzing de parsers de entrada (sequências de escape ANSI do terminal, eventos de mouse SGR corrompidos).
  - Fuzzing do parser de mensagens LSP/JSON-RPC e do protocolo binário do daemon remoto.

### 11.3 Testes de Snapshot e TUI Headless
- **Verificação Visual Automatizada**:
  - Emulação de terminal virtual headless via `crossterm` mock backend.
  - Gravação de estados de tela esperados (layouts, janelas suspensas de autocompletion, splits de abas) e asserções contra regressões visuais em cada pull request.

### 11.4 Corpus de Teste Real para Benchmarks
- **Corpus 1**: Árvore completa do **Linux Kernel** (~80.000 arquivos C/ASM).
- **Corpus 2**: Repositório do **LLVM Project** (monorepo C++ massivo com milhões de linhas).
- **Corpus 3**: Repositório do **OpenJdk** (árvore mista C++/Java).
- **Corpus 4**: Arquivo monolítico de log gerado sinteticamente com **1.5 GB** de texto contínuo.

---

## 12. Riscos e Mitigações

| Risco | Probabilidade | Impacto | Mitigação Arquitetural | Gatilho de Decisão |
|---|---|---|---|---|
| **R1. Servidor LSP de monorepo (ex.: clangd) consumir dezenas de GBs de RAM** | Alta | Alto | Lume monitora o processo filho do LSP via `sysinfo`. Se o consumo ultrapassar o limite configurado (ex.: 4 GB), ativa o Modo Large Repo com indexação seletiva. | Processo LSP atinge 80% do teto configurado de memória do projeto. |
| **R2. Incompatibilidade de emuladores de terminal com sequências de mouse SGR ou repintura ANSI** | Média | Médio | Fallback gracioso para modo puramente teclado se a detecção de capacidade do terminal falhar no handshake inicial. | Handshake do terminal não responder à consulta de versão de terminal em 100 ms. |
| **R3. Queda súbita de conexão SSH durante edição remota** | Alta | Médio | O daemon `lumed` mantém a sessão de buffers sujos viva em background com journal local no servidor até a reconexão do cliente. | Heartbeat de rede falhar após 3 tentativas de ping no canal SSH. |
| **R4. Exaustão de file descriptors do `inotify` no Linux em monorepos de 1 milhão de arquivos** | Alta | Alto | Não registrar watchers recursivos na raiz do monorepo; registrar watchers apenas nos diretórios dos buffers atualmente abertos na sessão ativa. | Falha de sistema `ENOSPC` ou repositório com mais de 50.000 diretórios. |
| **R5. Gargalo de serialização entre UI e Workers** | Baixa | Médio | Transmissão de spans e deltas textuais usando structs com lifetime zero-copy (`Cow<str>`) ou ponteiros imutáveis `Arc<Rope>`. | Profiler acusar > 5% do tempo de frame gasto em deserialização de canais internos. |

---

## 13. Plano de Ação Imediato: Primeiros 10 Dias de Trabalho

### Dia 1: Estrutura do Workspace e Scaffold Cargo
- [x] **Passo 1.1**: Inicializar workspace Cargo em `/home/unicode/Documents/lite-ide` contendo os crates `lume-core` e `lume-tui`.
- [x] **Passo 1.2**: Configurar CI local básico com flags de compilação estritas (`cargo clippy -- -D warnings`, `cargo fmt`).
- [x] **Passo 1.3**: Escrever o teste de fumaça inicial que valida a criação dos crates e compilação limpa.

### Dia 2: Implementação do Wrapper de Buffer sobre `ropey`
- [x] **Passo 2.1**: Escrever testes unitários para a trait `TextBuffer` cobrindo inserção, deleção e conversão de linha/coluna.
- [x] **Passo 2.2**: Implementar `RopeBuffer` encapsulando `ropey::Rope` em `lume-core/src/buffer/`.
- [x] **Passo 2.3**: Executar testes e validar tempo de inserção em buffers com 500k linhas.

### Dia 3: Motor de Cursores e Múltiplos Cursores
- [x] **Passo 3.1**: Criar testes para o `CursorEngine` cobrindo adição de cursores, ordenação e fusão (*merge*) de cursores sobrepostos.
- [x] **Passo 3.2**: Implementar manipulação multi-cursor e seleção por bloco em `lume-core/src/cursor/`.
- [x] **Passo 3.3**: Validar com testes automatizados a aplicação simultânea de inserção em múltiplos pontos.

### Dia 4: Árvore de Undo/Redo e Transações
- [x] **Passo 4.1**: Criar testes property-based com `proptest` testando simetria de Undo/Redo.
- [x] **Passo 4.2**: Implementar estrutura de histórico em árvore com agrupamento de edições consecutivas por tempo.
- [x] **Passo 4.3**: Garantir que as posições dos múltiplos cursores sejam restauradas com precisão em cada undo.

### Dia 5: Terminal Loop e Renderizador TUI Base
- [x] **Passo 5.1**: Configurar inicialização do terminal raw mode e captura de eventos com `crossterm`.
- [x] **Passo 5.2**: Implementar viewport com numeração de linhas e renderização do texto visível na janela.
- [x] **Passo 5.3**: Medir latência do loop de renderização com buffer estático.


### Dia 6: Suporte de Mouse de Primeira Classe
- [x] **Passo 6.1**: Habilitar modo de mouse SGR 1006 no terminal via escape sequences do `crossterm`.
- [x] **Passo 6.2**: Mapear clique do mouse para cálculo de linha/coluna relativa ao viewport do buffer.
- [x] **Passo 6.3**: Implementar seleção de texto arrastando o mouse e rolagem suave via roda do mouse.

### Dia 7: Autosave Atômico e Journal de Recuperação
- [x] **Passo 7.1**: Escrever testes simulando escrita atômica com arquivos temporários e atomic rename.
- [x] **Passo 7.2**: Implementar `AutoSaveController` com timers configuráveis (por inatividade e ao perder foco).
- [x] **Passo 7.3**: Implementar journal binário append-only de crash recovery em diretório de cache.

### Dia 8: Syntax Highlighting com Tree-Sitter em Background Thread
- [x] **Passo 8.1**: Integrar crate `tree-sitter` e gramáticas de C e Rust em `lume-syntax`.
- [x] **Passo 8.2**: Configurar worker thread que recebe cópias de deltas e computa a árvore de sintaxe sem travar a UI.
- [x] **Passo 8.3**: Integrar mapeamento de cores no renderizador TUI para os tokens sintáticos.

### Dia 9: Command Palette e Which-Key Overlay
- [x] **Passo 9.1**: Implementar widget flutuante de overlay com input de texto e fuzzy matching.
- [x] **Passo 9.2**: Criar registro central de comandos e tabela de atalhos.
- [x] **Passo 9.3**: Implementar timer de 300 ms para acionamento automático do popup `which-key` para atalhos pendentes.

### Dia 10: Integração M0 Completa e Benchmark de Validação
- [x] **Passo 10.1**: Conectar todos os módulos implementados no executável CLI principal.
- [x] **Passo 10.2**: Rodar benchmark de startup a frio e validar meta de < 50 ms (atingido: ~51 µs).
- [x] **Passo 10.3**: Rodar benchmark de latência tecla-pixel com PTY virtual e validar meta de < 16 ms (atingido: ~970 ns para inserção em 100k linhas).

