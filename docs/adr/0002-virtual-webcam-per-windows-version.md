# Virtual webcam uses Media Foundation on Windows 11 and DirectShow on Windows 10

On Windows 11 the Virtual webcam is a Media Foundation virtual camera (`MFCreateVirtualCamera`), because it is the only mechanism visible to every app, including Media Foundation-only ones such as the Windows Camera app, while Frame Server still exposes it to DirectShow apps. That API does not exist on Windows 10, so there Spiegel falls back to a DirectShow source filter (the approach OBS uses), which Media Foundation-only apps cannot see. Only one of the two is registered on a given machine, chosen by Windows version, so apps never list the same Device twice.

## Consequences

- Both mechanisms are COM DLLs registered under HKLM, so installing the Virtual webcam needs administrator rights.
- The Media Foundation source runs inside the Frame Server service, not in Spiegel's process, so Spiegel needs its own IPC (e.g. shared memory) to hand it frames. No official sample covers this. It is the riskiest piece and should be prototyped first. The prototype (issue #1, branch `spike/mf-virtual-webcam`) confirmed it works: ~1 ms from Spiegel's process to the consuming app, working in OBS, Zoom, Meet (including the remote participant), Slack, Chrome, Edge and the Windows Camera app.
- Windows lets only one app use a virtual camera at a time; a second app opening it concurrently fails. This is platform behaviour, not ours to fix.
- A normal user cannot create the cross-session (`Global\`) shared memory; only the media source, running as LocalService inside Frame Server, can. The media source creates it and Spiegel waits for it (ADR 0004).
