# Instruções para agentes

Este arquivo orienta os agentes de IA que trabalham com o código deste repositório. O `AGENTS.md` é um link simbólico para o `CLAUDE.md`, então edite o `CLAUDE.md`.

## Idioma

O idioma padrão do projeto é o **português do Brasil (pt-BR)**:

- **Em pt-BR**: conversas com o mantenedor, issues, specs, tickets, ADRs, o `CONTEXT.md`, os READMEs, comentários no código e mensagens de commit.
- **Em inglês**: identificadores no código (tipos, funções, variáveis, módulos), seguindo a convenção de Rust e TypeScript. Use os nomes de código indicados no `CONTEXT.md` (por exemplo, Sessão → `Session`).
- **Interface do Spiegel**: pt-BR e inglês, seguindo o idioma do sistema. Se o idioma do sistema não for suportado, a interface usa inglês.
- **READMEs**: o `README.md` é em pt-BR e o `README.en.md` é a versão em inglês.

## Projeto

O Spiegel é uma interface gráfica para o [scrcpy](https://github.com/Genymobile/scrcpy), que espelha e controla Android pelo adb.

## Estrutura e comandos

- `crates/spiegel-core/`: o núcleo em Rust (adb, registro de Dispositivos e, em breve, protocolo do scrcpy e Sessões). Não depende do Tauri. A API pública dele é a costura de teste, e o adb falso fica em `testing` (feature `testing`).
- `src-tauri/`: a casca Tauri 2, que só liga o núcleo à interface por comandos e eventos.
- `src/`: a interface em React + TypeScript (Vite). Os textos ficam em `src/i18n/locales/` (pt-BR e inglês, com as mesmas chaves).
- `scripts/fetch-platform-tools.mjs`: baixa o adb embutido para `src-tauri/resources/platform-tools/`, que fica fora do git.

Pré-requisitos: Rust (stable), Node 22+ e pnpm.

```sh
pnpm install
pnpm fetch:adb                  # uma vez: baixa o adb embutido (o build falha sem ele)
pnpm tauri dev                  # roda o aplicativo (Vite na porta 1420)
cargo test --workspace          # testes do núcleo e da casca
pnpm test                       # testes da interface (vitest)
pnpm build                      # checagem de tipos + build da interface
pnpm lint                       # oxlint
cargo run -p spiegel-core --example watch_devices   # lista de Dispositivos ao vivo no terminal, com o adb real
```

O que já foi decidido (veja o `CONTEXT.md` para o vocabulário e `docs/adr/` para os motivos):

- O Spiegel é um cliente próprio do scrcpy. Ele envia um `scrcpy-server` embutido, fixado na v4.1, e fala o protocolo ele mesmo, em vez de rodar o `scrcpy.exe` (ADR 0001).
- Stack: Tauri 2, com um núcleo em Rust (adb, protocolo, Sessões e decodificação FFmpeg para a Webcam virtual) e uma interface web em TypeScript que decodifica a prévia com WebCodecs. Quadros crus nunca passam pelo IPC do Tauri (ADR 0003).
- A Webcam virtual é uma DLL COM em C++: câmera virtual do Media Foundation no Windows 11 e filtro DirectShow no Windows 10 (ADR 0002).
- Windows primeiro, portável para Linux e macOS. A interface vem em pt-BR e inglês, com os textos em arquivos de tradução abertos a outros idiomas.
- Ordem de entrega:
  1. Protótipo descartável da Webcam virtual com Media Foundation (concluído, ADR 0004).
  2. Tela + Controle remoto por USB (spec na issue #4).
  3. Câmera como Fonte de vídeo, alimentando uma Gravação e a Webcam virtual.
  4. Perfis, Pareamento, áudio, i18n, instalador e bandeja do sistema.
- A Webcam virtual anuncia formatos fixos, independentes do Dispositivo: 720p30, 1080p30 e 1080p60, mais 2160p30 atrás de uma opção desligada por padrão, todos NV12 16:9. O núcleo Rust adapta cada quadro da Sessão ao formato que o aplicativo escolheu. Por padrão ele corta para 16:9, com barras pretas como opção do Perfil, e os Perfis de webcam pedem 16:9 ao Dispositivo (`--camera-ar=16:9`), então o corte raramente é necessário. Para gravar em 4K para o YouTube, o caminho recomendado é a Gravação, não a Webcam virtual.

## Referência do scrcpy

Há um clone local do scrcpy em `D:\apps\community\scrcpy` (tag v4.1). Use-o como fonte da verdade sobre o comportamento do scrcpy em vez de adivinhar. Bons pontos de partida:

- `app/src/cli.c`: a tabela `options[]` (cerca de 108 opções longas), com o nome, o argumento e o texto de ajuda de cada flag. É a lista canônica do que a interface pode expor.
- `app/src/options.h` / `options.c`: a struct `scrcpy_options` e os valores padrão por trás dessas flags.
- `doc/*.md`: documentação por área (video, audio, control, keyboard, mouse, camera, virtual-display, recording, connection, tunnels, otg, window, shortcuts, windows).
- `app/data/bash-completion`, `app/data/zsh-completion`: listas compactas de opções, úteis para conferência.
- `doc/develop.md` (seção "Protocol"), `app/src/control_msg.h`, `app/src/device_msg.h` e os testes `app/tests/test_control_msg_serialize.c` / `test_device_msg_deserialize.c`: o protocolo cliente/servidor e os bytes exatos de cada mensagem.

Fatos relevantes sobre o scrcpy:

- O scrcpy é um cliente em C/SDL mais um servidor Java (`scrcpy-server`), que o cliente envia para `/data/local/tmp/scrcpy-server.jar` no Dispositivo e roda pelo adb.
- O cliente encontra o adb pela variável de ambiente `ADB` e o arquivo do servidor por `SCRCPY_SERVER_PATH` (veja `app/src/adb/adb.c` e `app/src/server.c`).
- Opções de descoberta, que imprimem informações e saem, úteis para preencher seletores na interface: `--list-displays`, `--list-cameras`, `--list-camera-sizes`, `--list-encoders`, `--list-apps`. A seleção de Dispositivo é `--serial` / `-s`, e a configuração sem fio é `--tcpip` (porta adb padrão 5555).

## Agent skills

### Issue tracker

As issues ficam no GitHub Issues de juniormartinxo/spiegel e são gerenciadas com o `gh`. Veja `docs/agents/issue-tracker.md`.

### Triage labels

Usa as cinco labels de triagem padrão: needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix. Veja `docs/agents/triage-labels.md`.

### Domain docs

Um único contexto: um `CONTEXT.md` e `docs/adr/` na raiz do repositório. Veja `docs/agents/domain.md`.
