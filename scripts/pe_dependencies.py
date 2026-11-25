"""Read PE imports without executing the file or depending on a developer SDK."""
from pathlib import Path
import struct


def imports(path):
    data = Path(path).read_bytes()
    assert data[:2] == b'MZ'
    pe, = struct.unpack_from('<I', data, 0x3c)
    assert data[pe:pe + 4] == b'PE\0\0'
    machine, sections = struct.unpack_from('<HH', data, pe + 4)
    assert machine == 0x8664, f'Expected x64 PE: {path}'
    optional_size, = struct.unpack_from('<H', data, pe + 20)
    optional = pe + 24
    magic, = struct.unpack_from('<H', data, optional)
    assert magic == 0x20b
    image_base, = struct.unpack_from('<Q', data, optional + 24)
    headers = optional + optional_size
    def offset(rva):
        for index in range(sections):
            size, address, raw_size, raw_offset = struct.unpack_from('<IIII', data, headers + index * 40 + 8)
            if address <= rva < address + max(size, raw_size):
                return raw_offset + rva - address
        raise ValueError(f'Unmapped RVA {rva:x}: {path}')
    def name(rva):
        start = offset(rva)
        return data[start:data.index(b'\0', start)].decode('ascii').lower()
    result = set()
    for directory, stride, name_index in [(1, 20, 3), (13, 32, 1)]:
        rva, size = struct.unpack_from('<II', data, optional + 112 + directory * 8)
        if not rva or not size:
            continue
        start = offset(rva)
        for entry in range(start, start + size, stride):
            values = struct.unpack_from('<' + 'I' * (stride // 4), data, entry)
            if not any(values):
                break
            value = values[name_index]
            if directory == 13 and not values[0] & 1:
                value -= image_base
            result.add(name(value))
    return result


# Windows 10/11 inbox libraries. VC redistributables and third-party DLLs are
# deliberately excluded, even if the build VM happens to have them installed.
SYSTEM = set('''advapi32.dll avrt.dll bcrypt.dll bcryptprimitives.dll cfgmgr32.dll comctl32.dll
comdlg32.dll crypt32.dll cryptbase.dll d3d11.dll d3d12.dll d3d9.dll dcomp.dll
dbghelp.dll dnsapi.dll dwmapi.dll dxgi.dll gdi32.dll hid.dll imm32.dll iphlpapi.dll
kernel32.dll kernelbase.dll mpr.dll msvcrt.dll mswsock.dll ncrypt.dll netapi32.dll
normaliz.dll ntdll.dll ole32.dll oleaut32.dll opengl32.dll powrprof.dll propsys.dll
psapi.dll rpcrt4.dll secur32.dll setupapi.dll shell32.dll shcore.dll shlwapi.dll
ucrtbase.dll uiautomationcore.dll user32.dll userenv.dll usp10.dll uxtheme.dll version.dll winhttp.dll
wininet.dll winmm.dll winspool.drv wintrust.dll wldap32.dll ws2_32.dll wtsapi32.dll
wsock32.dll mf.dll mfplat.dll mfreadwrite.dll mfuuid.dll strmiids.dll'''.split())


def system(name):
    return name in SYSTEM or name.startswith(('api-ms-win-', 'ext-ms-win-'))


def audit(folder):
    files = {p.name.lower(): p for p in Path(folder).iterdir() if p.suffix.lower() in ('.exe', '.dll')}
    report = {}
    for name, path in sorted(files.items()):
        required = imports(path)
        missing = [item for item in sorted(required) if item not in files and not system(item)]
        assert not missing, f'{name} needs unbundled libraries: {missing}'
        report[name] = sorted(required)
    return report


def copy_graphics(source, destination):
    import shutil
    files = {p.name.lower(): p for p in Path(source).glob('*.dll')}
    pending = ['opengl32.dll', 'libgallium_wgl.dll']
    copied = set()
    while pending:
        name = pending.pop()
        if name in copied:
            continue
        if name not in files:
            assert system(name), f'Mesa dependency is missing: {name}'
            continue
        shutil.copy2(files[name], Path(destination) / files[name].name)
        copied.add(name)
        pending.extend(imports(files[name]))
    assert 'opengl32.dll' in copied and 'libgallium_wgl.dll' in copied
    return sorted(copied)
