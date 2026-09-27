# SPIKE: Virtual webcam do Media Foundation alimentada por outro processo

> Código descartável. **Não** é a DLL de produção. Vive só no branch `spike/mf-virtual-webcam`.

**Pergunta.** No Windows 11, um processo separado, em modo usuário, consegue enviar quadros para uma câmera virtual do Media Foundation (`MFCreateVirtualCamera`)? E ela aparece e funciona em aplicativos reais?

**Resposta curta.** Sim. A media source roda dentro do Frame Server (`svchost -k Camera`, LocalService, sessão 0) e lê quadros NV12 de uma seção `Global\` escrita pelo alimentador, em modo usuário. Do alimentador até o app a latência é de ~1 ms, e a câmera funciona no app Câmera, no Chrome e no Edge. O OBS, o Zoom, o Discord e o Teams ainda precisam ser testados à mão (roteiro abaixo).

Ambiente: Windows 11 Pro 25H2 (build 26200), VS 2022 Build Tools 17.14, Windows SDK 10.0.26100, sem câmera física.

## Montagem

| Peça | Arquivo | Onde roda |
|---|---|---|
| Media source (DLL COM, CLSID `{6E2F4C1B-9A3D-4E57-B0C8-2D7A15F3E901}`) | `source/vcam_source.cpp` | Frame Server (sessão 0) **e** em cada app consumidor |
| Alimentador (padrão de teste 1280×720 NV12 @ 30 fps + janela com relógio) | `feeder/feeder.cpp` | sessão do usuário |
| `vcamctl` (add/remove/list/dshow/probe) | `vcamctl/vcamctl.cpp` | sessão do usuário |
| Probe de navegador (getUserMedia + carimbo) | `browser/` | Chrome/Edge |
| Contrato da memória compartilhada | `common/shared_frame.h` | ambos |

- Memória compartilhada `Global\SpiegelSpikeVCamFrames`: cabeçalho de 4 KiB + 2 slots NV12 com seqlock, mais o evento `Global\SpiegelSpikeVCamFrameReady`. A DACL (SDDL) libera SYSTEM, LocalService, Administradores, Usuários autenticados e Interativo.
- A source oferece NV12 (primeiro) e YUY2. Converte NV12→YUY2 na CPU. Ignora `SetD3DManager` (amostras em memória de sistema) e usa o alocador fornecido pelo Frame Server.
- `RequestSample` só enfileira o token. Uma thread entrega quando chega um quadro novo (evento, mais um poll de 5 ms). Sem alimentador (heartbeat com mais de 500 ms), entrega a imagem de espera a 30 fps, cadenciada por um waitable timer de alta resolução.
- Cada quadro leva um carimbo binário (QPC de 64 bits + número do quadro + checksum) nas linhas de cima. Com ele, `vcamctl probe` e `browser/probe.html` medem a latência exata.

## Como rodar

```powershell
cmake -S . -B build -G "Visual Studio 17 2022" -A x64; cmake --build build --config Release
# como administrador (copia para Program Files, registra em HKLM, cria a pasta de logs):
pwsh -File scripts\install.ps1
# como usuário comum:
build\Release\vcamctl.exe add --lifetime session --access user   # a camera vive enquanto este processo viver
build\Release\vcam-feeder.exe                                    # padrao de teste + janela de relogio
build\Release\vcamctl.exe probe --format nv12 --seconds 10       # ou --format yuy2; --no-set; --name <parte do nome>
node browser\serve.mjs 8765   # e abra http://127.0.0.1:8765/ no navegador
# limpeza (admin):
pwsh -File scripts\uninstall.ps1
```

Os logs da DLL ficam em `C:\ProgramData\SpiegelVCamSpike\logs\<processo>-<pid>.log`.

## Resultados

### 1. Aplicativos

| App | Resultado | Como foi testado |
|---|---|---|
| Consumidor Media Foundation (`vcamctl probe`, IMFSourceReader) | ✅ NV12 e YUY2, 30,1 fps, 0 perdas | automático |
| App Câmera do Windows | ✅ imagem ao vivo | UI Automation + screenshot |
| Chrome 154 (getUserMedia) | ✅ 1280×720 @ 30 fps, 0 perdas | headless, `browser/probe.html` |
| Edge 154 | ✅ 1280×720 @ 30 fps, 0 perdas | headless, `browser/probe.html` |
| Alimentador morre e volta com a câmera aberta | ✅ ~0,5 s congelado (limite do heartbeat) → espera → ao vivo de novo, sem reabrir | `vcamctl probe` + `taskkill` |
| DirectShow (enumeração) | ✅ aparece com YUY2 e NV12 1280×720 @ 30 | `vcamctl dshow` |
| OBS 32.1 "Dispositivo de captura de vídeo" | ✅ 1280×720 com formato "Qualquer", YUY2 e NV12, **também com a source oferecendo só NV12** (o YUY2 vem da ponte) | manual, pelo usuário |
| Zoom 7.1.9 | ✅ prévia em Configurações → Vídeo (nome longo truncado: "Spiegel Spike Webcam (Câmera Virtual d...") | manual, screenshot do usuário |
| Discord | ⏳ pendente (manual) | |
| Teams (e o outro lado da chamada) | ⏳ pendente. O Teams não está instalado; testar no teams.microsoft.com ou instalar | |
| webcamtests.com | ⏳ pendente (manual) | |

### 2. Latência

| Trecho | p50 | faixa |
|---|---|---|
| alimentador publica → media source enfileira a amostra | 0,5 ms | — |
| alimentador → `ReadSample` num app MF, NV12 | 1,1 ms | 0,7–3,1 ms (p95 1,7) |
| alimentador → `ReadSample`, YUY2 | 1,3 ms | 1,0–4,0 ms (p95 1,9) |
| alimentador → quadro apresentado no Chrome (`expectedDisplayTime`, headless) | 27 ms | 21–63 ms |
| alimentador → quadro apresentado no Edge (idem) | 33 ms | 19–36 ms |
| alimentador → tela no app Câmera (screenshot com a janela de relógio ao lado) | ~31 ms | 17–49 ms (5 amostras) |
| alimentador → prévia de Configurações do Zoom (screenshot) | ~21 ms | 1 amostra |
| alimentador → OBS (screenshots) | 6–25 ms | 7 amostras em NV12/YUY2/Any; a prévia da janela de propriedades às vezes mostra um quadro atrás (~43 ms) |

O transporte entre processos custa ~1 ms. O resto vem da renderização do app e da idade do quadro (até 33 ms a 30 fps). O tempo até o primeiro quadro depois de abrir a câmera é de ~15–45 ms (`ActivateObject` ~30 ms).

### 3. Administrador e local da DLL

- **Precisa de admin:** registrar o CLSID em `HKLM\SOFTWARE\Classes\CLSID\...`, copiar a DLL para `Program Files`, parar/reiniciar o serviço `FrameServer` (necessário para trocar a DLL ou forçar uma nova instância da source) e `MFCreateVirtualCamera(..., MFVirtualCameraAccess_AllUsers, ...)` (o `Start` sem admin devolve `E_ACCESSDENIED`).
- **Não precisa de admin:** `MFCreateVirtualCamera` com `MFVirtualCameraAccess_CurrentUser`, tanto com `Lifetime_Session` quanto com `Lifetime_System` (este persiste depois que o processo sai), `Remove`, rodar o alimentador e consumir a câmera.
- **Local da DLL:** num lugar que LocalService (Frame Server) e LocalSystem (Frame Server Monitor) consigam ler. `C:\Program Files\<App>\` funciona (Usuários: RX). Um caminho dentro do perfil do usuário (`C:\Users\...`) não funciona, conforme o VCamSample. O `D:\` desta máquina também daria (Usuários: RX), mas não foi testado. Como a DLL também é carregada nos processos dos apps, incluindo apps empacotados/AppContainer, a pasta precisa dar RX para "TODOS OS PACOTES DE APLICATIVOS", como o `Program Files` já dá.

### 4. Armadilhas

1. **Usuário comum não cria objeto `Global\`.** `CreateFileMapping("Global\...")` falha com erro 5 (sem `SeCreateGlobalPrivilege`). Quem cria a seção é a media source (LocalService tem o privilégio), e o alimentador só *abre*. Consequência: o alimentador não consegue publicar até algum app abrir a câmera pela primeira vez. Para a produção: criar a seção num serviço, ou a partir do instalador/serviço, ou aceitar "Spiegel espera a câmera ser aberta". Além disso, uma DACL explícita é obrigatória, senão a sessão do usuário não abre um objeto criado por LocalService.
2. **Local\ não serve.** O Frame Server está na sessão 0, e o alimentador na sessão do usuário.
3. **A DLL é carregada em vários processos.** Carregam a DLL o Frame Server, o processo que chama `MFCreateVirtualCamera`/`Start` (ele ativa a source para validá-la) e cada app consumidor, onde o MF ativa e desliga uma instância local. Consequências: construtores leves; a DLL não pode ser sobrescrita enquanto algum desses processos estiver vivo (o instalador renomeia a DLL em uso); e uma DLL só x64 deixa de fora apps de 32 bits, o que não foi testado.
4. **O Frame Server mantém uma instância longeva da source.** Mudar os formatos oferecidos (ou a DLL) só vale depois de `Restart-Service FrameServer` (admin), mesmo removendo e recriando a câmera.
5. **Resolução do timer no serviço.** `WaitForMultipleObjects(..., 5)` vira ~15,6 ms dentro do svchost, e a imagem de espera saía a 21,7 fps. Resolvido com `CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`: agora sai a 30,2 fps.
6. **A ponte DirectShow sintetiza YUY2.** Com a source oferecendo *só* NV12, o `vcamctl dshow` ainda lista YUY2 + NV12, e o OBS funciona em YUY2, NV12 e Any. **A DLL de produção pode oferecer só NV12**, sem a conversão na CPU. Com NV12 + YUY2 a ponte lista YUY2 duas vezes. Em "Device Default", o OBS escolheu 960×720, uma resolução que a source não oferece (a ponte escala).
7. **O Windows acrescenta o sufixo ao nome.** O dispositivo aparece como "Spiegel Spike Webcam (Câmera Virtual do Windows)", com o sufixo localizado. O CONTEXT.md fala em "sempre sob o mesmo nome": o nome base é nosso, mas o sufixo não.
8. **Registro duplicado.** Chamar `MFCreateVirtualCamera` com lifetime/acesso diferentes cria outro dispositivo com o mesmo nome. Os apps passam a listar duas "Spiegel". O produto precisa registrar exatamente uma combinação.
9. **Processo morto não remove a câmera de sessão na hora.** Depois de um `taskkill` no `vcamctl add --lifetime session`, o dispositivo continuou listado. Ao recriá-lo, o symlink se repetiu (mesmo ID de dispositivo), então não duplicou.
10. **Carimbo e croma.** Um detalhe só do teste: o carimbo precisa de croma neutro por baixo, senão a conversão YUV→RGB do navegador o corrompe.
11. **Um app por vez.** Com um consumidor aberto (OBS, ou um `vcamctl probe`), um segundo app simultâneo falha no `ReadSample` com `0xC00D3704` (`MF_E_HW_MFT_FAILED_START_STREAMING`), mesmo sem chamar `SetCurrentMediaType`. O Frame Server nem chega a chamar a source para o segundo cliente. **A outra câmera virtual MF desta máquina ("JM-S21") se comporta igual**, então é da plataforma, não da DLL, apesar de `MF_DEVICESTREAM_FRAMESERVER_SHARED=1`. Consequência para a Spiegel: a Virtual webcam serve um app de cada vez (não dá OBS e Zoom juntos). Não testei o modo `SharedReadOnly` do `MediaCapture` (WinRT).
12. **Teams, o outro lado da chamada.** O VCamSample registra que a prévia aparece mas o outro participante não recebe vídeo. Ainda não verificado aqui.

## Roteiro dos testes manuais

Com `vcamctl add` e `vcam-feeder` rodando (a janela verde de relógio fica sempre no topo):

1. Abra o app, escolha **"Spiegel Spike Webcam"** e confira se o padrão se move e o `FRAME` sobe.
2. Latência: tire um screenshot (Win+Shift+S) com a janela de relógio e a imagem da câmera visíveis. Latência = relógio verde − `T=` da imagem.
3. Feche o alimentador (Ctrl+C): em até ~0,5 s deve aparecer "STANDBY - SEM SINAL". Abra de novo e a imagem ao vivo deve voltar sem reabrir a câmera no app.
4. OBS: *Dispositivo de captura de vídeo* → Spiegel, testando o formato "Qualquer", YUY2 e NV12.
5. Teams: numa chamada com outra pessoa (ou outra conta/dispositivo), confirme que **o outro lado vê** o padrão.
