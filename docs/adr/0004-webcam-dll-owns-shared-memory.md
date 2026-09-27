# The Windows 11 webcam DLL creates the shared memory; Spiegel waits for it

On Windows 11 the Virtual webcam's media source runs inside the Frame Server service in session 0, so the frames it reads from Spiegel must live in a cross-session (`Global\`) shared-memory section. Windows only lets processes with `SeCreateGlobalPrivilege` create such a section, and a normal user does not have it. So the media source (which runs as LocalService and has it) creates the section with an explicit DACL the moment an app opens the camera, and Spiegel keeps retrying to open it (about every 0.5 s) and only starts publishing frames once it can. We chose this because nobody sees the camera before an app opens it anyway: the cost is at most ~0.5 s of standby image at the start, and in exchange Spiegel ships no extra component. The prototype (issue #1) runs exactly this way.

## Consequences

- Spiegel cannot publish frames before some app opens the Virtual webcam, so it must treat "section not there yet" as normal, not as an error.
- The media source must not tear the section down while Spiegel holds it open; the section lives as long as either side holds a handle.

## Considered options

- **A Windows service that creates the section at boot**: Spiegel could publish at any time, but it is one more component to install, update and keep out of the portable (Linux/macOS) build, for no visible benefit.
- **Granting `SeCreateGlobalPrivilege` to users from the installer**: changes the machine's security policy for a marginal gain.
