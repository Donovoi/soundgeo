import math, pathlib, struct, subprocess, time, wave, zipfile
root = pathlib.Path.cwd()
dest = root / 'fixture-diagnostic'
dest.mkdir()
with zipfile.ZipFile(root / 'runtime-artifact/windows-runtime.zip') as stream:
    (dest / 'ffmpeg.exe').write_bytes(stream.read('ffmpeg.exe'))
ffmpeg = str(dest / 'ffmpeg.exe')
image = dest / 'frame.ppm'
image.write_bytes(b'P6\n160 90\n255\n' + bytes([16,25,40]) * (160 * 90))
audio = dest / 'audio.wav'
with wave.open(str(audio), 'wb') as stream:
    stream.setnchannels(1)
    stream.setsampwidth(2)
    stream.setframerate(8000)
    stream.writeframes(b''.join(struct.pack('<h', int(2000*math.sin(i*math.pi/10))) for i in range(8000*30)))
for name, args in [
    ('silent', ['-loop','1','-framerate','1','-i',str(image),'-t','2','-c:v','mpeg4','-pix_fmt','yuv420p']),
    ('audio', ['-loop','1','-framerate','1','-i',str(image),'-i',str(audio),'-c:v','mpeg4','-pix_fmt','yuv420p','-c:a','aac','-b:a','96k','-shortest'])
]:
    print('START', name, flush=True)
    start = time.monotonic()
    result = subprocess.run([ffmpeg,'-nostdin','-hide_banner','-loglevel','error','-y',*args,str(dest/(name+'.mp4'))], timeout=45)
    result.check_returncode()
    print('PASS',name,round(time.monotonic()-start,2), flush=True)
