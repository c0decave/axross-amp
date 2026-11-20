# Show on display

Back to the [README](../README.md) - everything in detail: the [extended README](README-extended.md).

**Show on display** (⚙ → Show on display, or D): the pictures on a monitor
of their own - a TV over HDMI, a projector - full screen and borderless,
while the window stays as it is on its screen (the playlist and the
controls go on working; where the picture was, it says where it is
shown: the face is drawn once, there). The monitors are listed with their
connector (HDMI-1, DP-2, VGA-1), their own name where the system knows it
(EDID) and their mode. *With the sound*: the sound moves to that display's
HDMI or DisplayPort output - the sink (or the card's port, switching the
card to its profile) whose display name (the ELD the display sends:
`device.product.name`, a port's name in brackets) is the monitor's own
name, the same name and never a part of one (a display called "TV" is not
a "TV Soundbar"), never a Bluetooth sink; without names only where it
cannot be mistaken: the one HDMI output for the one display that carries
sound. Otherwise it says it cannot tell which output plays there (choose
it in the Mixer tab): a connector's number is no guide - DP-1 and HDMI-1
are both a card's first HDMI output, as NVIDIA's HDMI-0 and DP-0 are.
pactl and the wait for a switched card's output run on a thread of their
own (the window is never held). When the view closes the sound goes back
where it was - the output and the card's profile (on Intel HDA one card
is analog *or* HDMI: left on HDMI, the analog output was gone for every
program); a switch whose output does not come is undone at once. Quitting
with the view open (the close button, SIGTERM or Ctrl+C in its terminal)
puts them back too, before the program ends. What it
was is kept in axross-amp's own file before anything moves it, so it is
put back also after a restart (the view opened again does not take the
display's output for "before"), after a crash, or when the remembered
display is gone. VGA and DVI carry no sound, and it says so. In the view:
Escape closes it, V / Shift+V change the face, Space plays and pauses, and
the track's title shows for a few seconds when it changes, fading in and
out. A display unplugged closes its view (the sound back, said); one
plugged in is offered. The view opens again on the next start, on the
same display (by its connector, else its name). Its flash guard is given
that screen in points, as the faces are drawn (its pixels over the
desktop's scale - on a 2 x desktop not four times too large a field), and
Calm or Intense hold there as here. Limits: the monitors are asked of
`xrandr --current` on X11 (only while the menu is open, the view shown or
about to open again, and on D - no prober running all session) and of the
kernel's DRM connectors (`/sys/class/drm`) under Wayland (XWayland's
XWAYLAND0, XWAYLAND1 are never taken for displays); on Wayland a program
cannot place its window on an output - the compositor decides where the
full-screen view goes; the desktop's scale is taken as this window's
(one scale for all displays); macOS and Windows list no monitors here.
The second window itself has been checked only by hand-made monitor
lists in the tests (`tests/display.rs`: the menu, the sound to an HDMI
sink of axross's fake pactl and back, a card switched to HDMI and back,
a switch that fails, the sound back after a restart, unplugging, opening
again, the guard's size at 1 x and 2 x), not on a real second screen.
