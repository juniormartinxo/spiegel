# A Webcam virtual usa Media Foundation no Windows 11 e DirectShow no Windows 10

No Windows 11, a Webcam virtual é uma câmera virtual do Media Foundation (`MFCreateVirtualCamera`). É o único mecanismo visível para todos os aplicativos, inclusive os que só usam Media Foundation, como o aplicativo Câmera do Windows, e o Frame Server continua expondo a câmera para os aplicativos DirectShow. Essa API não existe no Windows 10, então lá o Spiegel recorre a um filtro de origem DirectShow (a abordagem do OBS), que os aplicativos que só usam Media Foundation não enxergam. Em cada máquina só um dos dois é registrado, conforme a versão do Windows, para que os aplicativos nunca listem a mesma câmera duas vezes.

## Consequências

- Os dois mecanismos são DLLs COM registradas em HKLM, então instalar a Webcam virtual exige permissão de administrador.
- A origem do Media Foundation roda dentro do serviço Frame Server, não no processo do Spiegel. Por isso o Spiegel precisa de um IPC próprio (por exemplo, memória compartilhada) para entregar os quadros. Nenhum exemplo oficial cobre isso, e por ser a parte de maior risco precisava de um protótipo antes de tudo. O protótipo (SPG-1, antiga issue #1 do GitHub, branch `spike/mf-virtual-webcam`) confirmou que funciona: cerca de 1 ms do processo do Spiegel até o aplicativo que usa a câmera, funcionando no OBS, Zoom, Meet (inclusive para o outro participante), Slack, Chrome, Edge e no aplicativo Câmera do Windows.
- O Windows só deixa um aplicativo usar uma câmera virtual de cada vez. Um segundo aplicativo que tente abri-la ao mesmo tempo falha. É um comportamento da plataforma, não algo que nos cabe resolver.
- Um usuário comum não consegue criar memória compartilhada entre sessões (`Global\`). Só a origem de mídia, rodando como LocalService dentro do Frame Server, consegue. A origem de mídia cria a memória e o Spiegel espera por ela (ADR 0004).
