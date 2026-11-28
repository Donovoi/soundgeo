"""Run only inside an isolated GitHub Windows VM; never execute on the host."""
import ctypes, hashlib, json, os, platform, subprocess, sys, time, traceback, urllib.request
from pathlib import Path
assert sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
ROOT = Path.cwd()
CACHE = ROOT / 'field-cache'
OUT = ROOT / 'field-evidence'
CACHE.mkdir(exist_ok=True)
OUT.mkdir(exist_ok=False)
EXE = CACHE / 'soundgeo-0.4.0-alpha.5-windows-x64.exe'
EXE_SHA = '334942971cb72104d031e134df138a30aefdc11f2183c5685c1ad32b5caf570a'
URL = 'https://www.youtube.com/watch?v=s3Fxioc-OxI'
PROFILE = ROOT / 'Isolated Windows Profile'
PROFILE.mkdir(exist_ok=False)
os.environ['LOCALAPPDATA'] = str(PROFILE)
os.environ['PATH'] = str(Path(os.environ['SystemRoot']) / 'System32')
PROOF = {'release': 'v0.4.0-alpha.5', 'source_commit': '6169da0f65aec8fc5fb648c8652044f9453d9aa1',
         'exe_sha256': EXE_SHA, 'source_url': URL, 'source_section_s': [1200,4800],
         'camera_position_supplied': False, 'recording_clock_supplied': False,
         'possible_date': '2026-11-17', 'execution': 'GitHub Windows VM', 'runs': []}

def sha(path):
    with path.open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()

def save():
    (OUT/'run.json').write_text(json.dumps(PROOF,indent=2),encoding='utf-8')

def run(name,args,timeout=180,allowed=(0,)):
    start=time.monotonic()
    print('Starting '+name,flush=True)
    with (OUT/(name+'.log')).open('x',encoding='utf-8') as log:
        p=subprocess.Popen([str(a) for a in args],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT)
        try: code=p.wait(timeout=timeout)
        except BaseException:
            subprocess.run(['taskkill','/PID',str(p.pid),'/T','/F'],capture_output=True)
            raise
    result={'stage':name,'exit_code':code,'elapsed_s':time.monotonic()-start}
    PROOF['runs'].append(result);save();print(json.dumps(result),flush=True)
    if code not in allowed: raise RuntimeError(name+' failed; see preserved log')
    return (OUT/(name+'.log')).read_text(encoding='utf-8',errors='replace')

def windows():
    u=ctypes.windll.user32
    found=[]
    callback_type=ctypes.WINFUNCTYPE(ctypes.c_bool,ctypes.c_void_p,ctypes.c_void_p)
    u.IsWindowVisible.argtypes=[ctypes.c_void_p]
    u.GetWindowTextW.argtypes=[ctypes.c_void_p,ctypes.c_wchar_p,ctypes.c_int]
    @callback_type
    def callback(hwnd,_):
        title=ctypes.create_unicode_buffer(1024)
        u.GetWindowTextW(hwnd,title,1024)
        if u.IsWindowVisible(hwnd) and title.value=='SoundGeo':found.append(hwnd)
        return True
    u.EnumWindows.argtypes=[callback_type,ctypes.c_void_p]
    u.EnumWindows(callback,0)
    return found

def screenshot(name):
    ps=OUT/'capture.ps1'
    if not ps.exists():
        ps.write_text('''Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$rect = [Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object Drawing.Bitmap $rect.Width,$rect.Height
$graphics = [Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size)
$bitmap.Save($args[0],[Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
''',encoding='utf-8')
    subprocess.run([str(Path(os.environ['SystemRoot'])/'System32/WindowsPowerShell/v1.0/powershell.exe'),'-NoProfile','-File',str(ps),str(OUT/(name+'.png'))],check=True,timeout=30)

def main():
    if not EXE.exists():
        req=urllib.request.Request('https://github.com/Donovoi/soundgeo/releases/download/v0.4.0-alpha.5/'+EXE.name,headers={'User-Agent':'SoundGeo-release-field-validation'})
        with urllib.request.urlopen(req,timeout=60) as response,EXE.open('xb') as f:
            while b:=response.read(1024*1024):f.write(b)
    assert sha(EXE)==EXE_SHA
    assert '0.4.0-alpha.5' in run('public-exe-version',[EXE,'--version'])
    extracted=ROOT/'Extracted Public Release'
    run('extract-public-exe',[EXE,'--portable-extract',extracted])
    product,=extracted.iterdir()
    build=json.loads((product/'BUILD.json').read_text(encoding='utf-8'))
    assert build['source_commit']==PROOF['source_commit']
    PROOF['bundled_build']=build
    PROOF['platform']=platform.platform()
    save()
    # Inspect the actual released desktop before media acquisition.
    with (OUT/'desktop-initial.log').open('x',encoding='utf-8') as log:
        gui=subprocess.Popen([str(EXE)],stdout=log,stderr=subprocess.STDOUT)
        try:
            deadline=time.monotonic()+90
            while not windows():
                assert gui.poll() is None
                assert time.monotonic()<deadline
                time.sleep(.25)
            hwnd=windows()[0]
            ctypes.windll.user32.ShowWindow(ctypes.c_void_p(hwnd),3)
            ctypes.windll.user32.SetForegroundWindow(ctypes.c_void_p(hwnd))
            time.sleep(2)
            screenshot('desktop-initial')
        finally:
            for hwnd in windows():ctypes.windll.user32.PostMessageW(ctypes.c_void_p(hwnd),0x0010,0,0)
            gui.wait(timeout=30)
    # Keep the original source interval fixed, independent of detector or search results.
    media=CACHE/'recording.mkv'
    if not media.exists():
        run('public-video-download',[product/'yt-dlp.exe','--ignore-config','--no-playlist','--socket-timeout','30','--retries','2','--fragment-retries','2',
            '--js-runtimes','deno:'+str(product/'deno.exe'),'--ffmpeg-location',product,
            '-f','bv*[height<=360]+ba/b[height<=360]/b','--download-sections','*1200-4800',
            '--merge-output-format','mkv','--remux-video','mkv','-o',str(CACHE/'recording.%(ext)s'),URL],timeout=1800)
    assert media.exists()
    PROOF['media_sha256']=sha(media)
    PROOF['media_bytes']=media.stat().st_size
    PROOF['status']='media_ready'
    save()
    run('audio-check',[EXE,'check-audio','--input',media,'--output',OUT/'audio-check.json'],timeout=600)
    run('media-probe',[product/'ffprobe.exe','-v','error','-show_format','-show_streams','-of','json',media])
    # This first probe verifies acquisition and the public executable before GUI automation.
    PROOF['status']='intake_complete'
    save()

try:main()
except BaseException as error:
    PROOF['status']='failed'
    PROOF['error']=str(error)
    (OUT/'failure.txt').write_text(traceback.format_exc(),encoding='utf-8')
    save()
    raise
