#!/usr/bin/env python3
import os, tarfile, io, time, pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[1]
arch=sys.argv[1] if len(sys.argv)>1 else 'aarch64'
ver=root.joinpath('Cargo.toml').read_text().split('version = "',1)[1].split('"',1)[0]
dist=root/'dist'; dist.mkdir(exist_ok=True)
pkg=dist/f'pkg-{arch}'
if pkg.exists(): subprocess.check_call(['rm','-rf',str(pkg)])
(pkg/'DEBIAN').mkdir(parents=True)
bin_dir=pkg/'data/data/com.termux/files/usr/bin'; bin_dir.mkdir(parents=True)
target={'aarch64':'aarch64-linux-android','x86_64':'x86_64-linux-android'}[arch]
src=root/f'target/{target}/release/opencode-native'
subprocess.check_call(['install','-m755',str(src),str(bin_dir/'opencode-native')])
try: os.symlink('opencode-native', bin_dir/'ocn')
except FileExistsError: pass
control=f"Package: opencode-native\nVersion: {ver}\nSection: utils\nPriority: optional\nArchitecture: {arch}\nMaintainer: OpenCode Native Contributors\nDescription: Lightweight native OpenCode-style coding agent\n A small Rust-based native agent for direct device use (Termux/Android/Linux).\n"
(pkg/'DEBIAN/control').write_text(control)
ct=dist/f'control-{arch}.tar.gz'; dt=dist/f'data-{arch}.tar.gz'
with tarfile.open(ct,'w:gz') as t:
    b=control.encode(); ti=tarfile.TarInfo('control'); ti.size=len(b); ti.mtime=int(time.time()); ti.mode=0o644; t.addfile(ti,io.BytesIO(b))
with tarfile.open(dt,'w:gz') as t:
    t.add(str(pkg/'data/data'), arcname='data')
deb=dist/f'opencode-native_{ver}_{arch}.deb'
def ar_add(f,name,data:bytes):
    hdr=f"{name:<16}{int(time.time()):<12}0     0     100644  {len(data):<10}`\n".encode()
    f.write(hdr); f.write(data)
    if len(data)%2==1: f.write(b'\n')
with open(deb,'wb') as f:
    f.write(b'!<arch>\n')
    ar_add(f,'debian-binary',b'2.0\n')
    ar_add(f,'control.tar.gz',ct.read_bytes())
    ar_add(f,'data.tar.gz',dt.read_bytes())
print(deb)
