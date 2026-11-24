import ctypes, hashlib, json, subprocess, time
from pathlib import Path
folder=Path('preflight').resolve()
gui=folder/'soundgeo.exe'
cli=folder/'soundgeo-cli.exe'
print(json.dumps({'scope':'Display environment preflight, not new release qualification','source_run':44204778664,'gui_sha256':hashlib.sha256(gui.read_bytes()).hexdigest(),'version':subprocess.check_output([str(cli),'--version'],text=True).strip()}),flush=True)
user32=ctypes.windll.user32
callback_type=ctypes.WINFUNCTYPE(ctypes.c_bool,ctypes.c_void_p,ctypes.c_void_p)
user32.GetWindowThreadProcessId.argtypes=[ctypes.c_void_p,ctypes.POINTER(ctypes.c_ulong)]
user32.IsWindowVisible.argtypes=[ctypes.c_void_p]
user32.EnumWindows.argtypes=[callback_type,ctypes.c_void_p]
def has_window(pid):
    found=[]
    @callback_type
    def callback(handle,_):
        owner=ctypes.c_ulong()
        user32.GetWindowThreadProcessId(handle,ctypes.byref(owner))
        if owner.value==pid and user32.IsWindowVisible(handle): found.append(handle)
        return True
    user32.EnumWindows(callback,0)
    return bool(found)
with (folder/'desktop.log').open('wb') as log:
    process=subprocess.Popen([str(gui)],cwd=folder,stdout=log,stderr=subprocess.STDOUT)
    try:
        deadline=time.monotonic()+30
        while not has_window(process.pid):
            assert process.poll() is None,'Desktop exited before opening'
            assert time.monotonic()<deadline,'No visible window'
            time.sleep(.25)
        time.sleep(2)
        assert process.poll() is None
        print('Desktop visible and alive',flush=True)
    finally:
        if process.poll() is None: process.terminate()
        process.wait(timeout=15)
        print((folder/'desktop.log').read_text(encoding='utf-8',errors='replace'),flush=True)
