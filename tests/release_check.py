#!/usr/bin/env python3
"""Docker-Goo live-engine checks + guided UI checks. Python 3 standard library only.
Run: python3 tests/release_check.py
Report: docker-goo-report-<run>.json in the directory where you launch this script.
Only resources with this run's unique prefix are created/removed. No sudo/prune.
"""
import argparse, datetime, json, os, pathlib, platform, shutil, socket
import subprocess, sys, tempfile, time, urllib.request, uuid

parser=argparse.ArgumentParser(description=__doc__,formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument('--context',help='Existing Docker context; the same context must be used in the TUI')
parser.add_argument('--auto',action='store_true',help='Run engine checks only; UI checks are marked untested')
parser.add_argument('--keep',action='store_true',help='Keep this run’s fixtures for investigation; report includes exact cleanup commands')
a=parser.parse_args()
run=datetime.datetime.now().strftime('%Y%m%d-%H%M%S')+'-'+uuid.uuid4().hex[:4]
prefix='goo-check-'+run
folder=pathlib.Path(tempfile.mkdtemp(prefix='docker-goo-check-'))
report_path=pathlib.Path.cwd()/('docker-goo-report-'+run+'.json')
D=['docker']+(['--context',a.context] if a.context else [])
containers=[];volumes=[];networks=[];images=[]
report={'run':run,'platform':platform.platform(),'prefix':prefix,'fixture_directory':str(folder),'checks':[],
        'scope':'Engine checks use Docker CLI. Guided UI checks require the tester; skipped checks are NOT passes.',
        'cleanup_commands':[]}
compose_file=folder/'compose.yaml'

def save():report_path.write_text(json.dumps(report,indent=2))
def record(name,status,detail):
    report['checks'].append({'name':name,'status':status,'detail':str(detail)[-16000:]})
    print(f'[{status.upper()}] {name}',flush=True);save()
def docker(*args,timeout=120,check=True):
    p=subprocess.run(D+list(args),text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=timeout)
    if check and p.returncode:raise RuntimeError(' '.join(args)+'\n'+p.stdout[-8000:])
    return p.returncode,p.stdout

def check(name,fn):
    try:detail=fn();record(name,'pass',detail or 'OK');return True
    except Exception as e:record(name,'fail',repr(e));return False

def expect(value,message):
    if not value:raise AssertionError(message)
def state(name):return json.loads(docker('inspect',name)[1])[0]['State']['Status']
def wait_state(name,wanted):
    until=time.time()+20
    while time.time()<until:
        if state(name)==wanted:return wanted
        time.sleep(.5)
    raise AssertionError(f'{name}: expected {wanted}, got {state(name)}')
def make(name,image,*args):
    name=prefix+'-'+name;containers.append(name)
    docker('run','-d','--name',name,*args,image,*({'alpine:3.22':['sleep','86400'],'ubuntu:24.04':['bash'],'python:3.12-alpine':['sleep','86400']}.get(image,[])),timeout=180)
    return name

def manual(name,instructions):
    if a.auto or not sys.stdin.isatty():record(name,'untested',instructions);return
    print('\nUI CHECK — '+name+'\n'+instructions+'\n')
    ans=input('Result [p=pass, f=fail, s=skip]: ').strip().lower()
    note=input('Observation/error (Enter for none): ').strip()
    record(name,{'p':'pass','f':'fail','s':'skipped'}.get(ans,'untested'),note or instructions)

def freeport():
    with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]

def cleanup():
    commands=[]
    if compose_file.exists():commands.append(D+['compose','-f',str(compose_file),'down','-v'])
    commands += [D+['rm','-f',n] for n in containers]
    commands += [D+['volume','rm',n] for n in volumes]
    commands += [D+['network','rm',n] for n in networks]
    commands += [D+['image','rm',n] for n in images]
    import shlex
    report['cleanup_commands']=[shlex.join(c) for c in commands]
    if not a.keep:
        failures=[]
        for c in commands:
            try:
                p=subprocess.run(c,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=90)
                if p.returncode and not any(x in p.stdout.lower() for x in ['no such','not found','does not exist']):failures.append(p.stdout)
            except Exception as e:failures.append(str(e))
        record('Cleanup of this run only','fail' if failures else 'pass','\n'.join(failures) or 'Unique-prefix fixtures removed; downloaded base images/build cache retained.')
    else:record('Cleanup','skipped','--keep: use the exact cleanup_commands in this report when finished.')
    save();print('\nREPORT TO SEND BACK:\n'+str(report_path))

print('Docker-Goo live check\nCreates isolated, uniquely named test containers, volumes, networks and a Compose project.')
print('Downloads nginx, Alpine, Ubuntu, Python and Redis images if needed. Uses localhost ports only.')
print('No broad prune, no changes to existing containers. --keep retains fixtures; default cleans this run only.')
print('Host files used only inside: '+str(folder))
if not a.auto and sys.stdin.isatty() and input('Type RUN to begin: ').strip()!='RUN':sys.exit('Canceled before creating resources.')
if not shutil.which('docker'):sys.exit('Docker CLI missing. Install it and run again.')
try:
    docker('version');report['docker_version']=docker('version','--format','{{json .}}')[1]
    report['context']=a.context or docker('context','show')[1].strip()
    try:report['docker_goo_version']=subprocess.check_output(['docker-goo','--version'],text=True).strip()
    except Exception:report['docker_goo_version']='not found on PATH'
    goo=shutil.which('docker-goo')
    if goo:
        import hashlib
        report['docker_goo_sha256']=hashlib.sha256(pathlib.Path(goo).read_bytes()).hexdigest()
        help_text=subprocess.check_output([goo,'--help'],text=True)
        record('Updated binary on PATH','pass' if '--console' in help_text and '--cwd' in help_text else 'fail',goo)
    else:record('Updated binary on PATH','fail','Install Docker-Goo before UI checks.')
    save()
    for image in ['nginx:alpine','alpine:3.22','ubuntu:24.04','python:3.12-alpine','redis:7-alpine']:
        if not check('Pull '+image,lambda im=image:docker('pull',im,timeout=900)[1]):raise RuntimeError('Resolve image pull failure, then rerun.')
    net=prefix+'-net';vol=prefix+'-data';networks.append(net);volumes.append(vol)
    docker('network','create',net);docker('volume','create',vol)
    web=make('web','nginx:alpine','--network',net,'-p','127.0.0.1::80')
    tools=make('tools','alpine:3.22','--network',net,'-v',vol+':/data','-e','GOO_TEST=hello world')
    ubuntu=make('ubuntu','ubuntu:24.04','-it','--network',net)
    metrics=make('metrics','python:3.12-alpine','--network',net)
    record('Fixture creation','pass',{'web':web,'tools':tools,'ubuntu':ubuntu,'metrics':metrics,'network':net,'volume':vol})
    check('Environment and sh exec',lambda:expect('hello world' in docker('exec',tools,'printenv','GOO_TEST')[1],'Wrong environment value'))
    check('Bash exec',lambda:expect('bash' in docker('exec',ubuntu,'bash','-c','echo bash')[1],'Bash did not run'))
    def webcheck():
        info=json.loads(docker('inspect',web)[1])[0];port=info['NetworkSettings']['Ports']['80/tcp'][0]['HostPort']
        url='http://127.0.0.1:'+port
        report['web_url']=url
        # HTTP opening is local only; a remote Docker context must be checked manually.
        for attempt in range(15):
            try:
                with urllib.request.urlopen(url,timeout=2) as r:expect(r.status==200,'HTTP status');return url
            except Exception:
                if attempt==14:raise
                time.sleep(.5)
    check('Local published HTTP',webcheck)
    check('Live-service logs',lambda:docker('logs',web)[1])
    def networkcheck():
        docker('exec',tools,'wget','-q','-O','/dev/null','http://'+web+'/')
        docker('network','disconnect',net,tools)
        code,_=docker('exec',tools,'wget','-T','2','-q','-O','/dev/null','http://'+web+'/',check=False)
        expect(code!=0,'Disconnected network unexpectedly resolves service')
        docker('network','connect',net,tools)
        docker('exec',tools,'wget','-q','-O','/dev/null','http://'+web+'/');return 'DNS, disconnect failure and reconnect succeed'
    check('Network connectivity',networkcheck)
    def lifecycle():
        docker('stop',web);wait_state(web,'exited');docker('start',web);wait_state(web,'running');docker('restart',web);wait_state(web,'running')
    check('Stop / start / restart',lifecycle)
    def pause():
        try:docker('pause',web);wait_state(web,'paused')
        finally:docker('unpause',web,check=False)
        wait_state(web,'running')
    check('Pause / resume (kernel support required)',pause)
    def persistence():
        docker('exec',tools,'sh','-c','printf proof > /data/proof.txt')
        docker('stop',tools);docker('rm',tools)
        docker('run','-d','--name',tools,'--network',net,'-v',vol+':/data','-e','GOO_TEST=hello world','alpine:3.22','sleep','86400')
        expect(docker('exec',tools,'cat','/data/proof.txt')[1].strip()=='proof','Volume lost data')
    check('Named volume survives recreation',persistence)
    check('Readonly mount',lambda:expect(docker('run','--rm','-v',vol+':/data:ro','alpine:3.22','touch','/data/blocked',check=False)[0]!=0,'Read-only write unexpectedly succeeded'))
    check('Non-forced deletion protection',lambda:expect(docker('rm',web,check=False)[0]!=0,'Running container removed without force'))
    def stats():
        docker('exec','-d',metrics,'python','-c',"import time; data=bytearray(32*1024*1024); end=time.monotonic()+8; exec('while time.monotonic()<end: pass')")
        return docker('stats','--no-stream','--format','{{json .}}',metrics,web,tools,timeout=30)[1]
    check('CPU/RAM/network stats under short load',stats)
    manual('Console editing, paste, history and progress',f'Open a second terminal: docker-goo --console --cwd {folder}\nUse the same Docker context as this report. Check docker is first suggestion. Paste: printf first-result. Execute, then a second command; both outputs and ==== barriers should remain. Test Left/Right, Delete, Home/End and Backspace. Try cd /tmp, pwd, ls; cd back to {folder}. Run docker pull nginx:alpine and confirm incremental output. Test wheel and PgUp/PgDn. Ctrl+S toggles suggestions; arrows choose and gradient highlights the current suggestion.')
    manual('Container UI, native -it and Ctrl+D',f'In Containers select {ubuntu}. b opens the shell chooser. Enter opens API shell; Ctrl+D returns. The Native terminal (-it) button opens the current terminal; exit returns. In $ try docker exec -it {ubuntu} bash; run pwd and exit. Ctrl+O opens host shell; exit returns. Check logs/stats on {web} and {metrics}; w opens the web port.')
    manual('Create form editing, paste and retained errors',f'Create a container through n, image alpine:3.22, name {prefix}-form, Service command sleep 86400. First set Ports to bad:80. Submit: error must stay visible with fields intact. Fix by clearing Ports, submit again, verify running. Check highlighted picker, fields/Create and paste. Delete this disposable container afterward.')
    containers.append(prefix+'-form')
    manual('Home/help mouse clicks and themes','Click [h] - Home and [?] - Help in the bottom drawer, then close Help. Test three themes, resize, and inspect console text/cursor contrast. Verify no duplicate logo captions.')
    if check('Compose plugin',lambda:docker('compose','version')[1]):
        compose_file.write_text(f'''name: {prefix}-compose
services:
  web:
    image: nginx:alpine
    ports:
      - "127.0.0.1::80"
  redis:
    image: redis:7-alpine
    command: ["redis-server", "--appendonly", "yes"]
    volumes: ["data:/data"]
volumes:
  data: {{}}
''')
        def composecheck():
            docker('compose','-f',str(compose_file),'up','-d',timeout=180)
            for _ in range(20):
                code,out=docker('compose','-f',str(compose_file),'exec','-T','redis','redis-cli','ping',check=False)
                if code==0 and 'PONG' in out:break
                time.sleep(.5)
            docker('compose','-f',str(compose_file),'exec','-T','redis','redis-cli','SET','goo','persisted')
            docker('compose','-f',str(compose_file),'down')
            docker('compose','-f',str(compose_file),'up','-d',timeout=180)
            for _ in range(20):
                code,out=docker('compose','-f',str(compose_file),'exec','-T','redis','redis-cli','GET','goo',check=False)
                if code==0 and 'persisted' in out:return 'Compose up/down and Redis volume persistence verified'
                time.sleep(.5)
            raise AssertionError('Redis persistence value missing')
        check('Compose lifecycle/persistence',composecheck)
        manual('Compose TUI and local file suggestions',f'Run docker-goo --console --cwd {folder}. Turn suggestions on. Type docker compose -f and a trailing space: compose.yaml should appear; select it then append up -d. Use Compose panel y with the same absolute file path, select the project, Enter to monitor members. Verify logs, stats, Inspect and web access.')
    (folder/'index.html').write_text('<h1>Docker-Goo check</h1>')
    (folder/'Dockerfile').write_text('FROM nginx:alpine\nCOPY index.html /usr/share/nginx/html/index.html\n')
    tag=prefix+':test';images.append(tag)
    check('Build and image inspect',lambda:(docker('build','-t',tag,str(folder),timeout=600)[1],docker('image','inspect',tag)[1]))
    def bindcheck():
        bind=make('bind','nginx:alpine','-v',str(folder)+':/usr/share/nginx/html:ro,Z')
        expect('Docker-Goo check' in docker('exec',bind,'cat','/usr/share/nginx/html/index.html')[1],'Bind file missing')
        expect(docker('exec',bind,'touch','/usr/share/nginx/html/blocked',check=False)[0]!=0,'Readonly bind unexpectedly writable')
        return 'Dedicated host fixture directory mounted read-only; SELinux private label requested'
    check('Host bind mount and readonly protection',bindcheck)
    def imagetag():
        temp_tag=prefix+':temporary';images.append(temp_tag)
        docker('tag',tag,temp_tag);docker('image','inspect',temp_tag);docker('image','rm',temp_tag)
        expect(docker('image','inspect',temp_tag,check=False)[0]!=0,'Temporary tag still exists')
        docker('image','inspect',tag)
    check('Image tagging and removal',imagetag)

    manual('Dockerfile suggestions',f'In $: cd {folder}; type docker build -f and a trailing space. Dockerfile must appear highlighted. Completing it must insert the file, not execute immediately. Add -t {prefix}:test . to execute if desired.')
except KeyboardInterrupt:
    record('Interrupted','blocked','User interrupted the test run.')
except Exception as e:
    record('Setup/suite interruption','fail',repr(e))
finally:
    cleanup()
