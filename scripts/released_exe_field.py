"""Run only inside an isolated GitHub Windows VM; never execute on the host."""
import ctypes, hashlib, json, os, platform, subprocess, sys, time, traceback, urllib.request
from pathlib import Path
assert sys.platform == 'win32' and os.environ.get('GITHUB_ACTIONS') == 'true'
ROOT = Path.cwd()
CACHE = ROOT / 'field-cache'
OUT = ROOT / 'field-evidence'
CACHE.mkdir(exist_ok=True)
OUT.mkdir(exist_ok=False)
CANDIDATE = os.environ.get('FIELD_CANDIDATE')=='1'
VERSION = '0.4.0-alpha.6' if CANDIDATE else '0.4.0-alpha.5'
EXE = (ROOT/'candidate-release' if CANDIDATE else CACHE)/('soundgeo-'+VERSION+'-windows-x64.exe')
EXE_SHA = '75c37b5ea296663671d81c37b1113f4ffe2e8d9431284bddf33ba4a9820ac556' if CANDIDATE else '334942971cb72104d031e134df138a30aefdc11f2183c5685c1ad32b5caf570a'
URL = 'https://www.youtube.com/watch?v=s3Fxioc-OxI'
PROFILE = ROOT / 'Isolated Windows Profile'
PROFILE.mkdir(exist_ok=False)
os.environ['LOCALAPPDATA'] = str(PROFILE)
os.environ['PATH'] = str(Path(os.environ['SystemRoot']) / 'System32')
PROOF = {'release': 'v'+VERSION, 'source_commit': '6623c3d26c7a6eb3a95483ea56857420d9c90608' if CANDIDATE else '6169da0f65aec8fc5fb648c8652044f9453d9aa1',
         'candidate_regression_only':CANDIDATE,
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

def uia(name,action='snapshot',media=None,**selectors):
    path=OUT/(name+'.json')
    args=[str(Path(os.environ['SystemRoot'])/'System32/WindowsPowerShell/v1.0/powershell.exe'),'-NoProfile','-File',str(ROOT/'scripts/released_exe_uia.ps1'),'-Action',action,'-OutputPath',str(path)]
    if media:args+=['-MediaPath',str(media)]
    for key,value in selectors.items():args+=['-'+key,str(value)]
    subprocess.run(args,check=True,timeout=30)
    return json.loads(path.read_text(encoding='utf-8-sig'))

def control(label,action,**selectors):
    for attempt in range(12):
        rows=uia(label+'-reveal-'+str(attempt),'reveal',**selectors)
        found=[r for r in rows if all(not value or r[{'MatchName':'name','MatchHelp':'help','MatchValue':'value','MatchType':'type'}[key]]==value for key,value in selectors.items() if key.startswith('Match'))]
        row=found[int(selectors.get('Ordinal',0))]
        if 90<=row['box'][1] and row['box'][1]+row['box'][3]<=690:break
        time.sleep(.3)
    else:raise RuntimeError('Could not scroll control into view: '+label)
    if label=='analysis-start':
        current=uia('verified-analysis-inputs')
        values=[row.get('value') for row in current]
        assert all(value in values for value in ('2026-11-17','LAX: public airport replay','Flight passage timing')), values
        assert any(row.get('help')=='Unknown — infer from flyovers' and not row.get('value') for row in current)
    uia(label,action,**selectors)
    time.sleep(.5)
    if label.endswith('-select'):
        uia(label+'-close-popup','escape')
    if action=='set':
        current=uia(label+'-verify')
        values=[row.get('value') for row in current]
        expected=selectors['NewValue']
        if selectors.get('MatchType')=='ControlType.Spinner':
            assert any(value is not None and abs(float(value)-float(expected))<1e-6 for value in values if value and value.replace('-','',1).replace('.','',1).isdigit()), (label,expected,values)
        else:assert expected in values,(label,expected,values)

def select_combo(label,current,desired):
    control(label+'-open','invoke',MatchValue=current)
    # Accessibility activation avoids popup movement during physical scrolling.
    uia(label+'-select','activate',MatchName=desired)
    time.sleep(1)
    uia(label+'-close-popup','escape')
    time.sleep(.5)
    rows=uia(label+'-selected-verify')
    assert any(row.get('type')=='ControlType.ComboBox' and row.get('value')==desired for row in rows), (label,desired)

def desktop_check(media):
    with (OUT/'desktop-recording.log').open('x',encoding='utf-8') as log:
        gui=subprocess.Popen([str(EXE)],stdout=log,stderr=subprocess.STDOUT)
        try:
            deadline=time.monotonic()+90
            while not windows():
                assert gui.poll() is None and time.monotonic()<deadline
                time.sleep(.25)
            hwnd=windows()[0]
            ctypes.windll.user32.ShowWindow(ctypes.c_void_p(hwnd),3)
            ctypes.windll.user32.SetForegroundWindow(ctypes.c_void_p(hwnd))
            time.sleep(2)
            select_combo('provider','Worldwide: ADSB.lol','LAX: public airport replay')
            control('date-set','set',MatchHelp='First date, or blank to find dates',NewValue='2026-11-17')
            select_combo('method','Doppler curves','Flight passage timing')
            control('bounds-open','invoke',MatchName='Search area & terrain')
            control('bounds-enable','invoke',MatchName='Limit the search area')
            for index,value in enumerate(('33.76','34.12','-118.65','-118.15')):
                control('bounds-'+str(index),'set',MatchType='ControlType.Spinner',Ordinal=index,NewValue=value)
            control('bounds-close','invoke',MatchName='Search area & terrain')
            screenshot('desktop-configured')
            control('media-entry','set',MatchHelp='Select a file',NewValue=str(media))
            for index in range(120):
                time.sleep(5)
                rows=uia('ui-check-'+str(index))
                text='\n'.join(row.get('name') or '' for row in rows)
                if index in (0,3,10,20,40):screenshot('desktop-check-'+str(index))
                if 'checked locally' in text:
                    screenshot('desktop-checked')
                    PROOF['desktop_automatic_check_completed']=True
                    save()
                    break
            else:raise RuntimeError('Desktop automatic check did not reach checked locally')
            if os.environ.get('FIELD_DESKTOP_INTAKE_ONLY')=='1':return
            cache=PROFILE/'SoundGeo/flight-data'
            assert not cache.exists(), 'Flight cache was populated before GUI analysis'
            PROOF['flight_cache_empty_before_gui_analysis']=True
            control('analysis-start','invoke',MatchName='Analyze recording')
            PROOF['desktop_analysis_started']=True
            save()
            started=time.monotonic()
            prior=''
            for index in range(540):
                time.sleep(10)
                rows=uia('ui-analysis-'+str(index))
                text='\n'.join(row.get('name') or '' for row in rows)
                if text!=prior:
                    with (OUT/'desktop-progress.jsonl').open('a',encoding='utf-8') as progress:
                        progress.write(json.dumps({'elapsed_s':time.monotonic()-started,'text':text})+'\n')
                    prior=text
                if index in (0,1,3,6,12,30,60,120,240):screenshot('desktop-analysis-'+str(index))
                if 'Export report…' in text:
                    screenshot('desktop-analysis-complete')
                    PROOF['desktop_analysis_completed']=True
                    PROOF['desktop_analysis_elapsed_s']=time.monotonic()-started
                    PROOF['desktop_result_snapshot']='ui-analysis-'+str(index)+'.json'
                    PROOF['flight_cache_after']=[{'name':str(p.relative_to(cache)),'bytes':p.stat().st_size} for p in cache.rglob('*') if p.is_file()]
                    PROOF['media_unchanged_after_analysis']=sha(media)==PROOF['media_sha256']
                    for manifest in cache.glob('*/provider-manifest.json'):
                        (OUT/('desktop-'+manifest.parent.name+'-manifest.json')).write_bytes(manifest.read_bytes())
                    save()
                    break
                if 'Cancel task' not in text and index>3:
                    screenshot('desktop-analysis-stopped')
                    raise RuntimeError('Desktop stopped without a report; see UI snapshot')
            else:raise RuntimeError('Desktop analysis exceeded 90 minutes')
        finally:
            for hwnd in windows():ctypes.windll.user32.PostMessageW(ctypes.c_void_p(hwnd),0x0010,0,0)
            gui.wait(timeout=30)

def main():
    if not EXE.exists():
        assert not CANDIDATE, 'Frozen candidate artifact is missing'
        req=urllib.request.Request('https://github.com/Donovoi/soundgeo/releases/download/v0.4.0-alpha.5/'+EXE.name,headers={'User-Agent':'SoundGeo-release-field-validation'})
        with urllib.request.urlopen(req,timeout=60) as response,EXE.open('xb') as f:
            while b:=response.read(1024*1024):f.write(b)
    assert sha(EXE)==EXE_SHA
    assert VERSION in run('public-exe-version',[EXE,'--version'])
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
            try:uia('ui-initial')
            except Exception as error:
                PROOF['initial_uia_error']=str(error)
                (OUT/'initial-uia-failure.txt').write_text(traceback.format_exc(),encoding='utf-8')
                save()
        finally:
            for hwnd in windows():ctypes.windll.user32.PostMessageW(ctypes.c_void_p(hwnd),0x0010,0,0)
            gui.wait(timeout=30)
    if os.environ.get('FIELD_UI_PROBE_ONLY')=='1':
        PROOF['status']='desktop_probe_complete'
        save()
        return
    # Keep the original source interval fixed, independent of detector or search results.
    media=CACHE/'recording.mkv'
    transfer_path=ROOT/'scripts/field-transfer.json'
    if not media.exists() and transfer_path.exists():
        transfer=json.loads(transfer_path.read_text())
        with urllib.request.urlopen(transfer['url'],timeout=120) as response,media.open('xb') as f:
            while chunk:=response.read(1024*1024):f.write(chunk)
        assert sha(media)==transfer['sha256']
        assert media.stat().st_size==transfer['bytes']
        PROOF['media_transfer']={'sha256_verified':True,'source':'Identical frozen public video downloaded in WSL VM; GitHub VM source request was challenged'}
    if not media.exists():
        run('public-video-download',[product/'yt-dlp.exe','--ignore-config','--no-playlist','--socket-timeout','30','--retries','2','--fragment-retries','2',
            '--js-runtimes','deno:'+str(product/'deno.exe'),'--ffmpeg-location',product,
            '-f','bv*[height<=360]+ba/b[height<=360]/b','--download-sections','*1200-4800',
            '--merge-output-format','mkv','--remux-video','mkv','-o',str(CACHE/'recording.%(ext)s'),URL],timeout=1800)
    assert media.exists()
    if transfer_path.exists():
        transfer=json.loads(transfer_path.read_text())
        assert sha(media)==transfer['sha256'] and media.stat().st_size==transfer['bytes']
    PROOF['media_sha256']=sha(media)
    PROOF['media_bytes']=media.stat().st_size
    PROOF['status']='media_ready'
    (CACHE/'media-verified.json').write_text(json.dumps({'sha256':PROOF['media_sha256'],'source_url':URL,'source_section_s':[1200,4800]},indent=2),encoding='utf-8')
    save()
    run('audio-check',[EXE,'check-audio','--input',media,'--output',OUT/'audio-check.json'],timeout=600)
    run('media-probe',[product/'ffprobe.exe','-v','error','-show_format','-show_streams','-of','json',media])
    try:desktop_check(media)
    except Exception as error:
        PROOF['desktop_check_error']=str(error)
        (OUT/'desktop-check-failure.txt').write_text(traceback.format_exc(),encoding='utf-8')
        save()
    if os.environ.get('FIELD_DESKTOP_ONLY')=='1':
        assert PROOF.get('desktop_automatic_check_completed'), 'Desktop intake failed'
        if os.environ.get('FIELD_DESKTOP_INTAKE_ONLY')!='1':assert PROOF.get('desktop_analysis_completed'), 'Desktop analysis failed'
        PROOF['status']='desktop_test_complete'
        save()
        return
    cache=PROFILE/'SoundGeo/flight-data'
    PROOF['flight_cache_empty_before_cli_analysis']=not cache.exists()
    job={'media':str(media),'bounds':{'south':33.76,'north':34.12,'west':-118.65,'east':-118.15},
         'analysis_method':'passage_timing','ground_m':0,'ground_datum':'egm96','offline':False,
         'archive':{'date_from':'2026-11-17','date_to':None,'cache':str(cache),'provider':'webtrak_lax'},'search_threads':2}
    (OUT/'job.json').write_text(json.dumps(job,indent=2),encoding='utf-8')
    save()
    run('full-analysis',[EXE,'analyze','--config',OUT/'job.json','--output',OUT/'report.json','--require-unambiguous'],timeout=5400,allowed=(0,3,4))
    PROOF['flight_cache_after']=[{'name':str(p.relative_to(cache)),'bytes':p.stat().st_size} for p in cache.rglob('*') if p.is_file()]
    PROOF['media_unchanged_after_analysis']=sha(media)==PROOF['media_sha256']
    report=json.loads((OUT/'report.json').read_text(encoding='utf-8'))
    PROOF['native_status']=report['status']
    if PROOF['flight_cache_after']:
        run('cached-offline-analysis',[EXE,'--offline','analyze','--config',OUT/'job.json','--output',OUT/'cached-report.json','--require-unambiguous'],timeout=1800,allowed=(0,3,4))
        cached=json.loads((OUT/'cached-report.json').read_text(encoding='utf-8'))
        PROOF['offline_cache_replay_status']=cached['status']
        PROOF['offline_cache_replay_candidates_equal']=cached.get('timing')==report.get('timing') and cached.get('candidates')==report.get('candidates')
    PROOF['status']='analysis_complete'
    save()
    # Separate technical suitability check only. Never run a location search on
    # the police appeal, infer whereabouts, identify people, or send a lead.
    appeal_url='https://www.abc.net.au/news/2026-07-05/cctv-footage-missing-dandenong-woman-yun/106881548'
    PROOF['appeal_scope']='Police-supplied public CCTV: download and audio suitability only; publication already describes the filming setting, so it is not an unidentified-location test.'
    run('appeal-intake',[EXE,'fetch-media','--url',appeal_url,'--cache',CACHE/'appeal-media','--output',OUT/'appeal-download.json'],timeout=600,allowed=(0,1))
    if (OUT/'appeal-download.json').exists():
        appeal=json.loads((OUT/'appeal-download.json').read_text(encoding='utf-8'))
        run('appeal-audio-check',[EXE,'check-audio','--input',appeal['path'],'--output',OUT/'appeal-audio-check.json','--require-searchable'],timeout=300,allowed=(0,1,3))
    PROOF['appeal_location_search_run']=False
    save()

try:main()
except BaseException as error:
    PROOF['status']='failed'
    PROOF['error']=str(error)
    (OUT/'failure.txt').write_text(traceback.format_exc(),encoding='utf-8')
    save()
    raise
