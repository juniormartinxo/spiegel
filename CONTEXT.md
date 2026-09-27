# Spiegel

O Spiegel é uma interface gráfica amigável para o scrcpy: tudo o que alguém faria com o scrcpy na linha de comando, feito por um único aplicativo, com a imagem do celular exibida dentro da própria janela do Spiegel.

Os termos abaixo valem para a documentação, as issues e a interface em pt-BR. No código, os nomes ficam em inglês. Cada termo traz o seu nome no código, e ele deve ser usado sempre, sem sinônimos.

## Linguagem

### Dispositivos e sessões

**Dispositivo** (código: `Device`):
Um celular ou tablet Android acessível pelo adb, via USB ou pela rede.
_Evite_: celular, aparelho, alvo

**Pareamento** (código: `Pairing`):
Autorizar um Dispositivo a usar o adb sem fio escaneando um QR code exibido pelo Spiegel, sem nenhum cabo.
_Evite_: vinculação, configuração sem fio

**Sessão** (código: `Session`):
Uma conexão ativa entre o Spiegel e um Dispositivo, que transmite o vídeo dele (e opcionalmente o áudio) e opcionalmente repassa a entrada do usuário. Várias Sessões podem rodar ao mesmo tempo, inclusive mais de uma no mesmo Dispositivo. Por exemplo, a Tela dele para o Controle remoto e a Câmera traseira para a Webcam virtual.
_Evite_: conexão, instância, espelhamento

**Perfil** (código: `Profile`):
Um conjunto de configurações de Sessão com nome e reutilizável (por exemplo, "Webcam traseira 1080p"). Iniciar uma Sessão é escolher um Dispositivo e um Perfil. O Spiegel vem com Perfis prontos, e cada Dispositivo lembra o último Perfil usado com ele.
_Evite_: preset, configuração, modelo

**Fonte de vídeo** (código: `VideoSource`):
O que uma Sessão transmite do Dispositivo: a **Tela** (código: `Screen`) ou uma das **Câmeras** (código: `Camera`).
_Evite_: entrada, feed

### Uso da câmera

**Webcam virtual** (código: `VirtualWebcam`):
Um dispositivo de câmera no computador, alimentado por uma Sessão cuja Fonte de vídeo é uma Câmera, que outros aplicativos (OBS, Zoom, Meet, navegadores) podem escolher como qualquer webcam física. Existe exatamente uma, sempre presente com o mesmo nome depois de instalada. Ela é alimentada por no máximo uma Sessão e usada por no máximo um aplicativo de cada vez. Sem nenhuma Sessão alimentando, mostra uma imagem de espera.
_Evite_: modo webcam, cam, saída de câmera

**Gravação** (código: `Recording`):
Um arquivo no disco do computador com o vídeo e o áudio de uma Sessão exatamente como o Dispositivo os codificou, sem nada gravado no armazenamento do Dispositivo e sem recompressão. É o jeito de maior qualidade de capturar uma Câmera, acima de qualquer coisa gravada pela Webcam virtual.
_Evite_: captura, clipe, exportação

### Entrada

**Controle remoto** (código: `RemoteControl`):
Comandar o Dispositivo pelo teclado e mouse do computador, para que um Dispositivo com a tela sensível ao toque danificada ou inutilizável continue totalmente usável.
_Evite_: modo de controle, repasse de entrada
