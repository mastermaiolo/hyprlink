import os, shutil
#!/usr/bin/env python3
"""Gera as pastas-perfil desta diretoria (`python3 gerar.py`). Os ficheiros
gerados estão versionados; este script existe para os poder refazer."""
root = os.path.dirname(os.path.abspath(__file__))
PERFIS = ['amd-so', 'intel-so', 'amd-nvidia-hibrido', 'nvidia-desktop', 'desktop-sem-bateria', 'duas-baterias']
for p in PERFIS:
    shutil.rmtree(f'{root}/{p}', ignore_errors=True)
def w(p, text):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p,'w').write(text if text.endswith('\n') or text=='' else text+'\n')

def base(prof, cpu_vendor, cpu_model, kernel='6.12.4-arch1-1', distro='Arch Linux'):
    d=f'{root}/{prof}'
    w(f'{d}/etc/os-release', f'NAME="{distro}"\nPRETTY_NAME="{distro}"\nID=arch')
    w(f'{d}/etc/hostname', 'pc-do-zeca')
    w(f'{d}/proc/sys/kernel/osrelease', kernel)
    w(f'{d}/proc/cpuinfo', f'processor\t: 0\nvendor_id\t: {cpu_vendor}\nmodel name\t: {cpu_model}\n')
    w(f'{d}/proc/meminfo', 'MemTotal:       16384000 kB\nMemAvailable:    8000000 kB\n')
    w(f'{d}/dev/.keep', '')
    return d

def proc(d, pid, comm, args=None):
    w(f'{d}/proc/{pid}/comm', comm)
    cmd = '\0'.join(args or [comm]) + '\0'
    os.makedirs(f'{d}/proc/{pid}', exist_ok=True)
    open(f'{d}/proc/{pid}/cmdline','w').write(cmd)

def hwmon(d, n, name, temps):
    h=f'{d}/sys/class/hwmon/hwmon{n}'
    w(f'{h}/name', name)
    for i,label,val in temps:
        if label: w(f'{h}/temp{i}_label', label)
        w(f'{h}/temp{i}_input', val)

def card(d, n, vendor, driver, busy=None):
    c=f'{d}/sys/class/drm/card{n}/device'
    w(f'{c}/vendor', vendor)
    if busy is not None: w(f'{c}/gpu_busy_percent', busy)
    os.makedirs(c, exist_ok=True)
    link=f'{c}/driver'
    if os.path.lexists(link): os.remove(link)
    os.symlink(f'../../../../bus/pci/drivers/{driver}', link)
    w(f'{d}/sys/class/drm/card{n}-eDP-1/.keep', '')

def supply(d, name, files):
    for k,v in files.items():
        w(f'{d}/sys/class/power_supply/{name}/{k}', v)

def meta(d, tools, hypr, vars_=None):
    w(f'{d}/tools.txt', '\n'.join(tools))
    w(f'{d}/hypr.txt', hypr)
    v={'HOME':'/home/zeca','USER':'zeca','XDG_SESSION_TYPE':'wayland','XDG_CURRENT_DESKTOP':'Hyprland'}
    if vars_: v.update(vars_)
    w(f'{d}/vars.txt', '\n'.join(f'{k}={x}' for k,x in v.items()))

common=['hyprctl','grim','slurp','wl-copy','wl-paste','loginctl','wpctl','pactl','gst-launch-1.0']

# 1. AMD + Noctalia v5
d=base('amd-so','AuthenticAMD','AMD Ryzen 5 5500U with Radeon Graphics')
hwmon(d,4,'k10temp',[('1','Tctl','61000'),('2','Tdie','51000')])
card(d,1,'0x1002','amdgpu','12')
supply(d,'ADP0',{'type':'Mains','online':'0'}); supply(d,'BAT0',{'type':'Battery','capacity':'63','status':'Discharging'})
proc(d,100,'pipewire'); proc(d,101,'wireplumber'); proc(d,200,'noctalia')
meta(d, common+['noctalia'], 'lua')

# 2. Intel + sem shell + hyprlock + Hyprland clássico
d=base('intel-so','GenuineIntel','Intel(R) Core(TM) i7-1165G7 @ 2.80GHz', kernel='6.6.50-1-lts')
hwmon(d,2,'coretemp',[('1','Package id 0','55000'),('2','Core 0','52000')])
hwmon(d,0,'acpitz',[('1','','40000')])
card(d,0,'0x8086','i915')
supply(d,'AC',{'type':'Mains','online':'1'}); supply(d,'BAT0',{'type':'Battery','capacity':'100','status':'Full'})
proc(d,100,'pipewire'); proc(d,101,'bash')
meta(d, common+['hyprlock','intel_gpu_top'], 'classic')

# 3. AMD + NVIDIA híbrido + Ryoku
d=base('amd-nvidia-hibrido','AuthenticAMD','AMD Ryzen 7 7840HS')
hwmon(d,3,'k10temp',[('1','Tctl','58000'),('2','Tdie','48000')])
card(d,0,'0x10de','nvidia'); card(d,1,'0x1002','amdgpu','5')
supply(d,'ACAD',{'type':'Mains','online':'1'}); supply(d,'BAT1',{'type':'Battery','capacity':'80','status':'Not charging'})
proc(d,100,'pipewire'); proc(d,300,'ryoku-shell')
meta(d, common+['nvidia-smi','ryoku-shell','upower'], 'lua')

# 4. NVIDIA desktop, PulseAudio, sem bateria, sem sensor bom (só acpitz)
d=base('nvidia-desktop','GenuineIntel','Intel(R) Core(TM) i9-12900K')
os.makedirs(f'{d}/sys/class/hwmon',exist_ok=True)
z=f'{d}/sys/class/thermal/thermal_zone0'
w(f'{z}/type','acpitz'); w(f'{z}/temp','38000')
card(d,0,'0x10de','nvidia')
supply(d,'AC',{'type':'Mains','online':'1'})
proc(d,100,'pulseaudio')
meta(d, ['hyprctl','grimblast','grim','hyprlock','loginctl','pactl','nvidia-smi'], 'lua')

# 5. Desktop AMD sem power_supply de todo
d=base('desktop-sem-bateria','AuthenticAMD','AMD Ryzen 9 5950X')
hwmon(d,1,'k10temp',[('1','Tctl','45000'),('3','Tdie','40000')])
card(d,0,'0x1002','amdgpu','3')
proc(d,100,'pipewire'); proc(d,400,'qs',['qs','-c','caelestia'])
meta(d, common+['caelestia'], 'lua')

# 6. Duas baterias do sistema + bateria de periférico (rato) com scope Device
d=base('duas-baterias','GenuineIntel','Intel(R) Core(TM) i5-8350U', kernel='6.1.100-1-lts')
hwmon(d,2,'coretemp',[('1','Package id 0','60000')])
card(d,0,'0x8086','i915')
supply(d,'AC',{'type':'Mains','online':'1'})
supply(d,'BAT0',{'type':'Battery','capacity':'40','status':'Charging'})
supply(d,'BAT1',{'type':'Battery','capacity':'90','status':'Discharging'})
supply(d,'hidpp_battery_0',{'type':'Battery','capacity':'12','status':'Discharging','scope':'Device'})
proc(d,100,'pipewire'); proc(d,500,'qs',['qs','-c','noctalia-shell'])
meta(d, common+['hyprlock'], 'lua')
