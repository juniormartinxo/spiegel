# Spiegel

Spiegel is a friendly graphical front-end for scrcpy: everything a user would do with scrcpy on the command line, done from one app, with the phone's image shown inside Spiegel's own window.

## Language

### Devices and sessions

**Device**:
An Android phone or tablet reachable through adb, over USB or the network.
_Avoid_: phone, handset, target

**Pairing**:
Authorizing a Device for wireless adb by scanning a QR code that Spiegel shows, with no cable involved.
_Avoid_: linking, wireless setup

**Session**:
One live connection between Spiegel and one Device, streaming its video (and optionally audio) and optionally forwarding the user's input. Several Sessions can run at once, including more than one on the same Device (e.g. its Screen for Remote control and its rear Camera for a Virtual webcam).
_Avoid_: connection, instance, mirror

**Profile**:
A named, reusable set of Session settings (e.g. "Rear camera webcam 1080p"). Starting a Session means picking a Device and a Profile. Spiegel ships built-in Profiles and each Device remembers the last one used with it.
_Avoid_: preset, config, template

**Video source**:
What a Session streams from the Device: either its **Screen** or one of its **Cameras**.
_Avoid_: input, feed

### Camera use

**Virtual webcam**:
A camera device on the computer, fed by a Session whose Video source is a Camera, that other apps (OBS, Zoom, Meet, browsers) can select like any physical webcam. There is exactly one, always present under the same name once installed, fed by at most one Session at a time; with none feeding it, it shows a standby image.
_Avoid_: webcam mode, cam, camera output

### Input

**Remote control**:
Driving the Device from the computer's keyboard and mouse, so a Device with a damaged or unusable touchscreen stays fully usable.
_Avoid_: control mode, input forwarding
