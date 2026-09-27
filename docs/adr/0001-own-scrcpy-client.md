# O Spiegel é um cliente próprio do scrcpy, não um invólucro do scrcpy.exe

O Spiegel envia o `scrcpy-server` oficial para o Dispositivo pelo adb e fala ele mesmo o protocolo cliente/servidor do scrcpy. Ele decodifica o vídeo, desenha dentro da própria janela, repassa a entrada de teclado e mouse e alimenta a Webcam virtual com os mesmos quadros. Escolhemos isso em vez de rodar o `scrcpy.exe` porque os dois requisitos principais, a imagem do celular dentro da janela do Spiegel e uma Webcam virtual no Windows, precisam dos quadros decodificados no processo do Spiegel. O `scrcpy.exe` não oferece nenhum dos dois: ele desenha na própria janela SDL, e a única saída de webcam dele é o V4L2, só no Linux.

## Consequências

- O protocolo é interno do scrcpy, e o servidor recusa um cliente de qualquer outra versão (`doc/develop.md`, seção "Protocol"). O Spiegel embute uma única versão do `scrcpy-server`, fixada na versão que ele implementa (v4.1 no início). Atualizar o scrcpy significa portar as mudanças do protocolo.
- Um scrcpy instalado pelo usuário nunca é usado. Só o adb é configurável: vem embutido por padrão, com um caminho personalizado opcional.

## Opções consideradas

- **Invólucro do `scrcpy.exe`**: é o mais barato, mas encaixar a janela SDL exige truques de reposicionamento de janela diferentes em cada sistema, e não dá acesso aos quadros para a Webcam virtual.
- **Híbrido** (invólucro para o espelhamento, cliente próprio para a webcam): duas implementações para o mesmo trabalho.
