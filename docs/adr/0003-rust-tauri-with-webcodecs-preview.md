# Rust + Tauri 2, com a prévia decodificada na webview e a Webcam virtual decodificada no código nativo

O Spiegel é um aplicativo Tauri 2. Um núcleo em Rust cuida do adb, do protocolo do scrcpy e das Sessões, e uma interface web em TypeScript desenha tudo o que o usuário vê. O caminho do vídeo é dividido para que nenhum quadro cru passe pelo IPC do Tauri. O núcleo repassa o fluxo ainda codificado (cerca de 1 a 2 MB/s) para a interface, que o decodifica com WebCodecs para a prévia dentro da janela. Quando a Webcam virtual está sendo alimentada, o núcleo também decodifica o mesmo fluxo com FFmpeg e escreve os quadros na memória compartilhada da DLL da webcam.

Escolhemos isso porque mantém uma interface web (o ecossistema mais rico para uma interface caprichada) e continua portável e sem gargalos. O WebCodecs vem ligado por padrão no WebView2, no WKWebView (Safari 16.4 ou mais recente) e no WebKitGTK 2.44 ou mais recente. Já os quadros crus em 1080p30 (cerca de 90 MB/s) passam do que o IPC do Tauri conseguiu transportar nas medições no Windows (cerca de 50 MB/s).

## Consequências

- Enquanto a Webcam virtual está sendo alimentada e a prévia está visível, cada quadro é decodificado duas vezes (na webview e no núcleo). Os decodificadores por hardware dão conta disso.
- O codec de vídeo padrão é H.264 ou H.265, porque o WKWebView não ativa a decodificação de AV1 por padrão.
- A DLL da Webcam virtual continua em C++ (COM do Windows), alimentada pelo núcleo Rust por memória compartilhada.

## Opções consideradas

- **Qt 6 (C++/QML)**: um único caminho nativo para os quadros, mas um kit de interface mais fraco e o protocolo escrito do zero em C++.
- **Electron ou Tauri com Tango (cliente scrcpy em TypeScript) fazendo toda a decodificação na webview**: a Webcam virtual precisaria de quadros crus copiados para fora da webview pelo IPC. Além disso, o suporte do Tango ao scrcpy 4.x ainda estava em beta.
- **Desenho nativo com wgpu embaixo de uma webview transparente**: não funciona no Linux com Wayland.
- **Media Source Extensions com MP4 fragmentado**: não há evidência de latência abaixo de 100 ms.
