"""Inspect only the frozen CI candidate in a Windows VM under a light theme."""
import ctypes,hashlib,json,os,subprocess,sys,time,winreg
from pathlib import Path
assert sys.platform=='win32' and os.environ.get('GITHUB_ACTIONS')=='true'
root=Path.cwd()
out=root/'theme-visual-evidence';out.mkdir(exist_ok=False)
exe=root/'candidate-release/soundgeo-0.4.0-alpha.6-windows-x64.exe'
receipt=json.loads((root/'candidate-release/validation-windows-x64.json').read_text())
with exe.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
assert digest==receipt['archive_sha256'] and receipt['passed']
# pull_request CI checks out GitHub's test-merge commit, whose tree is pinned below.
assert receipt['source_commit']=='6623c3d26c7a6eb3a95483ea56857420d9c90608'
assert digest=='75c37b5ea296663671d81c37b1113f4ffe2e8d9431284bddf33ba4a9820ac556'
assert receipt['source_tree']=='ac4baaedcefcf5fa00db684659733b451b49b022'
with winreg.CreateKey(winreg.HKEY_CURRENT_USER,r'Software\Microsoft\Windows\CurrentVersion\Themes\Personalize') as key:
    winreg.SetValueEx(key,'AppsUseLightTheme',0,winreg.REG_DWORD,1)
    winreg.SetValueEx(key,'SystemUsesLightTheme',0,winreg.REG_DWORD,1)
profile=root/'Isolated Theme Profile';profile.mkdir(exist_ok=False)
os.environ['LOCALAPPDATA']=str(profile)
os.environ['PATH']=str(Path(os.environ['SystemRoot'])/'System32')
ps=Path(os.environ['SystemRoot'])/'System32/WindowsPowerShell/v1.0/powershell.exe'
capture=out/'capture.ps1'
capture.write_text('''Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$rect=[Windows.Forms.SystemInformation]::VirtualScreen
$bitmap=New-Object Drawing.Bitmap $rect.Width,$rect.Height
$graphics=[Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size)
$bitmap.Save($args[0],[Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
''')
u=ctypes.windll.user32
cbtype=ctypes.WINFUNCTYPE(ctypes.c_bool,ctypes.c_void_p,ctypes.c_void_p)
u.EnumWindows.argtypes=[cbtype,ctypes.c_void_p]
u.IsWindowVisible.argtypes=[ctypes.c_void_p]
u.GetWindowTextW.argtypes=[ctypes.c_void_p,ctypes.c_wchar_p,ctypes.c_int]
def windows():
    found=[]
    @cbtype
    def visit(hwnd,_):
        title=ctypes.create_unicode_buffer(1024);u.GetWindowTextW(hwnd,title,1024)
        if u.IsWindowVisible(hwnd) and title.value=='SoundGeo':found.append(hwnd)
        return True
    u.EnumWindows(visit,0)
    return found
with (out/'desktop.log').open('x') as log:
    p=subprocess.Popen([str(exe)],stdout=log,stderr=subprocess.STDOUT)
    try:
        deadline=time.monotonic()+120
        while not windows():
            assert p.poll() is None and time.monotonic()<deadline
            time.sleep(.25)
        hwnd=ctypes.c_void_p(windows()[0]);u.ShowWindow(hwnd,3);u.SetForegroundWindow(hwnd)
        for delay in (2,15):
            time.sleep(delay)
            subprocess.run([str(ps),'-NoProfile','-File',str(capture),str(out/('light-system-'+str(delay)+'.png'))],check=True,timeout=30)
        (out/'receipt.json').write_text(json.dumps({'exe_sha256':digest,'source_commit':receipt['source_commit'],'source_tree':receipt['source_tree'],'windows_apps_light_theme':True,'execution':'GitHub Windows VM','screenshots_require_visual_review':True},indent=2))
    finally:
        for hwnd in windows():u.PostMessageW(ctypes.c_void_p(hwnd),0x0010,0,0)
        p.wait(timeout=30)
