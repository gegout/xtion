# Scoped investigation permissions

The existing `70-xtion.rules` grants read/write access to an enumerated
`1d27:0601` sensor. It does not grant rights to inspect root-only xHCI debugfs,
restart USB ports or reinitialize a shared controller. The current process can
already read the kernel journal; broad root shell access is unnecessary.

## One-time setup by the user

From a normal Ubuntu terminal, run:

```bash
sudo bash /home/cgegout/projects/xtion/packaging/grant-usb-investigation-access.sh
```

The installer copies three reviewed helpers to `/usr/local/libexec/xtion`, owned
by root and writable only by root. It creates `/etc/sudoers.d/xtion-investigation`
for the invoking account, after checking the policy with `visudo`. No USB device
is reset during installation.

The base grant permits exactly:

- Read-only USB hub descriptors, port status and xHCI debugfs diagnostics.
- A read-only guarded port-recovery plan and its explicit `--apply` operation.
- A read-only shared-controller recovery plan.

It does not grant the shared-controller mutation. To explicitly allow that
broader operation too, use:

```bash
sudo bash /home/cgegout/projects/xtion/packaging/grant-usb-investigation-access.sh --allow-controller-reset
```

This additional grant authorizes reinitializing Intel controller `0000:00:14.0`.
That temporarily disconnects the mouse, webcams, Bluetooth and associated hubs.
The helper attempts to restore the same driver afterward. Success is not assumed.

## Why the helpers are installed as root-owned copies

A passwordless sudo rule pointing to a script writable in the project would allow
that script to be replaced with arbitrary privileged code. These rules instead
name fixed, root-owned installed copies and fixed arguments. Python runs with
`-I`, which excludes the current script directory and user site packages and
ignores Python environment overrides. See the primary
[sudoers documentation](https://github.com/sudo-project/sudo/blob/main/docs/sudoers.man.in)
and [Python isolated-mode documentation](https://docs.python.org/3/using/cmdline.html#cmdoption-I).

Changes to the installed helpers require the user to rerun the installer. The
agent can then invoke only the permitted commands through `sudo -n`, without
asking for or receiving the user's password.

## Revoke

```bash
sudo rm /etc/sudoers.d/xtion-investigation
```

The root-owned helper files may remain; removing the policy removes their special
noninteractive execution grant. The ordinary Xtion device-access udev rule is
separate and remains in effect.

## Verification status

The installer passes `bash -n`; the generated policy parses with `visudo -cf`.
Six offline recovery tests pass. The installed policy was verified with
`sudo -n -l`, including the optional controller recovery entry. The installed
diagnostics helper ran without a password prompt. Ordinary user permissions
were sufficient for firmware queries, acquisition and a target-device reset.
