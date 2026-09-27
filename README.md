<div align="center">

# Spiegel

**Seu celular Android no computador: espelhe a tela, controle o aparelho e use a câmera dele como webcam.**

Uma interface gráfica amigável para o [scrcpy](https://github.com/Genymobile/scrcpy). Tudo o que você faria com o scrcpy na linha de comando, feito por um único aplicativo bonito.

[![Licença: MIT](https://img.shields.io/badge/licen%C3%A7a-MIT-blue.svg)](LICENSE)
![Status: desenvolvimento inicial](https://img.shields.io/badge/status-desenvolvimento%20inicial-orange.svg)
![Plataforma: Windows primeiro](https://img.shields.io/badge/plataforma-Windows%20primeiro-lightgrey.svg)

**Português (Brasil)** · [English](README.en.md)

</div>

> [!NOTE]
> O Spiegel está no início do desenvolvimento. O design está definido e a parte mais arriscada, a webcam virtual no Windows 11, já foi comprovada por um protótipo, mas ainda não existe um aplicativo pronto para uso. Dê uma estrela ou acompanhe o repositório para ver o progresso.

*Spiegel* significa "espelho" em alemão.

## Por quê

O scrcpy é rápido, leve e excelente, mas é uma ferramenta de linha de comando com umas 108 opções. O Spiegel coloca uma interface de verdade por cima dele e acrescenta o que o scrcpy não faz sozinho no Windows:

- **📷 O celular como webcam.** Use a câmera traseira ou frontal no OBS, Zoom, Google Meet, Slack, Chrome e em qualquer aplicativo que aceite webcam. Grave vídeos para o YouTube com uma câmera muito melhor que a do notebook.
- **🖱️ Controle total pelo teclado e mouse.** A tela do celular aparece dentro da janela do Spiegel e você comanda tudo pelo computador. É uma salvação quando a tela sensível ao toque está trincada ou parou de funcionar.
- **📱 Vários dispositivos e sessões ao mesmo tempo.** Por exemplo, espelhe a tela de um celular enquanto a câmera traseira dele alimenta a webcam.
- **💾 Perfis.** Salve configurações com um nome ("Webcam traseira 1080p", "Controle remoto") e inicie uma sessão com um clique. Perfis prontos cobrem os usos mais comuns.
- **📶 Conexão sem fio por QR code.** Pareie pelo Wi-Fi sem cabo (Android 11 ou mais recente), ou passe um celular conectado por USB para o Wi-Fi.
- **Nada para instalar no celular.** Assim como no scrcpy, basta ativar a depuração USB.

## Como funciona

O Spiegel é o seu próprio cliente do scrcpy. Em vez de abrir o `scrcpy.exe`, ele envia o `scrcpy-server` oficial para o celular via adb e conversa diretamente com ele. É isso que permite mostrar o celular dentro da própria janela e enviar o mesmo vídeo para uma webcam virtual.

```
Celular                       Computador
───────                       ──────────────────────────────────────────────────
Câmera / Tela                 Spiegel (núcleo em Rust)
  └─► scrcpy-server ──USB/Wi-Fi──► vídeo comprimido
      (codifica H.264/H.265)         ├─► decodificação FFmpeg ─► memória compartilhada ─► Webcam virtual ─► OBS, Zoom, Meet…
                                     └─► janela do Spiegel (WebCodecs) ─► prévia ao vivo + controle por teclado/mouse
```

| Parte | Tecnologia |
|---|---|
| Aplicativo e interface | [Tauri 2](https://tauri.app), interface web em TypeScript |
| Núcleo: adb, protocolo do scrcpy, sessões | Rust |
| Decodificação de vídeo | WebCodecs para a prévia, FFmpeg para a webcam |
| Webcam virtual | DLL COM em C++: Media Foundation no Windows 11, DirectShow no Windows 10 |
| Lado do celular | `scrcpy-server` oficial, fixo na v4.1 |

Os motivos de cada escolha estão registrados em [`docs/adr/`](docs/adr).

## Roadmap

- [x] **Design**: modelo de domínio e decisões de arquitetura
- [x] **Protótipo da webcam virtual (Windows 11)**: cerca de 1 ms do Spiegel até o aplicativo que usa a câmera. Funciona no OBS, Zoom, Google Meet (inclusive do outro lado da chamada), Slack, Chrome, Edge e no aplicativo Câmera do Windows.
- [ ] **Tela + controle remoto por USB**: o celular dentro da janela do Spiegel, comandado por teclado e mouse
- [ ] **Câmera do celular, gravada e como webcam**: grave direto no disco do computador, sem recompressão (até 4K, se o celular suportar), ou alimente a webcam virtual em 720p/1080p; câmera traseira ou frontal, fps, zoom, lanterna
- [ ] **Perfis, pareamento por QR code, áudio, instalador, bandeja do sistema**
- [ ] **Idiomas**: inglês e português primeiro, aberto a outros
- [ ] **Linux e macOS**

Ideias para depois incluem um microfone virtual, um modo de socorro guiado para celulares com a tela morta e a depuração USB ainda não autorizada, displays virtuais e suporte a gamepad.

## Requisitos (previstos)

- **Windows 11** para ter todos os recursos. O Windows 10 também é suportado, com a webcam via DirectShow, que aplicativos que só usam Media Foundation (como a Câmera do Windows) não enxergam.
- Um dispositivo Android com a **depuração USB** ativada e este computador autorizado. A câmera exige Android 12 ou mais recente, como no scrcpy.
- Permissão de administrador **uma única vez**, na primeira vez que você ativar a webcam virtual. Todo o resto é instalado só para o seu usuário.

O Windows permite que apenas um aplicativo por vez use uma câmera virtual.

## Documentação do projeto

- [`CONTEXT.md`](CONTEXT.md): o vocabulário do projeto (Dispositivo, Sessão, Perfil, Webcam virtual…)
- [`docs/adr/`](docs/adr): registros de decisões de arquitetura
- [`CLAUDE.md`](CLAUDE.md): notas para agentes de IA que trabalham no repositório

## Agradecimentos

O Spiegel se apoia no [scrcpy](https://github.com/Genymobile/scrcpy), da Genymobile e de Romain Vimont, licenciado sob Apache 2.0. O Spiegel é um projeto independente, sem vínculo com a Genymobile.

## Licença

[MIT](LICENSE) © 2026 Junior Martins
