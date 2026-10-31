#!/usr/bin/env python3
"""Verify real fixture states, native bounds, and IOSurface exports for every entry."""
import json,time,urllib.request,urllib.error
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
BASE='http://127.0.0.1:43221'
def request(path,method='GET'):
 with urllib.request.urlopen(urllib.request.Request(BASE+path,method=method),timeout=8) as r:return json.load(r)
def wait_info(id,predicate):
 until=time.monotonic()+4
 while True:
  info=request('/info?id='+id)
  if predicate(info):return info
  assert time.monotonic()<until,(id,info)
  time.sleep(.05)
entries=json.loads((ROOT/'catalog.json').read_text())
for attempt in range(50):
 try:
  before=request('/components');break
 except urllib.error.URLError:
  if attempt==49:raise
  time.sleep(.1)
pid=before['processID']
assert [e['id'] for e in before['catalog']]==[e['id'] for e in entries]
results=[]
try:
 for entry in entries:
  id=entry['id'];request('/focus?id='+id,'POST');request('/resize?id='+id+'&width=390','POST')
  info=wait_info(id,lambda i:i['selected']==id and (entry['renderer']!='GPUI' or i['state'].get('id')==id))
  if id=='terminal':info=wait_info(id,lambda i:'Production terminal emulator' in i['state'].get('terminal',''))
  assert info['processID']==pid and info['renderer']==entry['renderer'] and info['source']==entry['source']
  assert info['width']==390 and info['height']==entry['height']
  surface=request('/surface/start?id='+id,'POST')
  assert 'error' not in surface and surface.get('surfaceID',0)>0,(id,surface)
  assert surface['width']==round(390*info['scale']) and surface['height']==round(entry['height']*info['scale']),(id,surface)
  state=info['state'];labels='\n'.join(state.get('labels',[]))
  expected={'signed-out':'Connect to Tailscale','empty':'No computers are available','refreshing':'Refreshing Tailscale machines','roster-error':'Could not refresh','connection-invalid':'Enter your SSH username.','connection-key-copied':'Public key copied','saved-row':'Studio Mac','online-row':'Online in Tailscale','offline-row':'Offline in Tailscale','section-header':'COMPUTERS','device-settings':'THIS DEVICE','picker-footer':'DEVICE NAME'}.get(id)
  if expected:
   info=wait_info(id,lambda i:expected in "\n".join(i["state"].get("labels",[])))
   state=info["state"];labels="\n".join(state.get("labels",[]))
  if id=='search-results':
   assert state['fields']['machine-search']=='Build' and 'Build server' in labels and 'MacBook Pro' not in labels
   assert '1 result for “Build”' in labels and 'Choose a computer to connect over SSH.' in labels
   assert 'tailscale-device-name' not in state['fields']
   assert 'Save device name' not in labels and 'Sign out of Tailscale' not in labels
  if id=='search-empty':assert state['fields']['machine-search']=='missing-machine' and 'Build server' not in labels
  if id=='session-menu':
   assert state['menu'] and not state['tools_menu']
   assert state['sessions']==3 and state['active_session']==0
   assert [s['name'] for s in state['active_sessions']]==['Studio Mac','Build server','Dev box']
   assert [s['selected'] for s in state['active_sessions']]==[True,False,False]
  if id=='terminal-tools':assert state['tools_menu'] and not state['menu']
  if id=='connecting':assert state['busy'] and state['message']=='Connecting…'
  if id=='key-strip-control':assert state['control']
  results.append({'id':id,'renderer':entry['renderer'],'surface':surface['surfaceID'],'size':[surface['width'],surface['height']]})
 for id in ('machines','connection','terminal','key-strip'):
  request('/focus?id='+id,'POST');original=request('/info?id='+id)
  resized=request('/resize?id='+id+'&width=540','POST');surface=request('/surface/start?id='+id,'POST')
  assert resized['width']==540 and resized['hostIdentity']==original['hostIdentity'] and surface['width']==1080
  request('/resize?id='+id+'&width=390','POST')
 for width in ['nan','inf','-1','10000']:
  try:request('/resize?id=machines&width='+width,'POST')
  except urllib.error.HTTPError as e:assert e.code==422
  else:raise AssertionError('Invalid size accepted: '+width)
 report={'result':'passed','processID':pid,'entries':results,'nativeResizeChecks':4,'invalidBoundsRejected':True}
 (ROOT/'.build/catalog-check.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
finally:request('/focus?id=machines','POST')
