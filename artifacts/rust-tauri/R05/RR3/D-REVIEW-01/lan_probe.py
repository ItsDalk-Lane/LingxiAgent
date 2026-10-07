# Apple解释器对照：真实监听先完成，然后以相同地址执行四字节交换。
import os, select, socket, sys, time
deadline = time.monotonic() + 6
def ready(sock, write=False):
    remain = max(0, deadline-time.monotonic())
    r, w, _ = select.select([] if write else [sock], [sock] if write else [], [], remain)
    if not (w if write else r): raise TimeoutError('aggregate six-second deadline')
def four(sock, data=None):
    result = b''
    while len(result) < 4:
        ready(sock, data is not None)
        if data is not None:
            n = sock.send(data[len(result):])
            if n <= 0: raise RuntimeError('send returned no bytes')
            result += data[len(result):len(result)+n]
        else:
            part = sock.recv(4-len(result))
            if not part: raise RuntimeError('premature EOF')
            result += part
    return result
listener = socket.socket(); client = socket.socket(); peer = None; accepted = False; ok = False
try:
    listener.bind(('0.0.0.0', 0)); listener.listen(4)
    port = listener.getsockname()[1]
    print('pid={} bind=0.0.0.0:{} connect={}:{} listening=true'.format(os.getpid(), port, sys.argv[1], port), flush=True)
    client.settimeout(max(0.001,deadline-time.monotonic())); client.connect((sys.argv[1],port)); client.setblocking(False)
    print('client connected',flush=True); four(client,b'PING'); print('client send=4',flush=True)
    ready(listener); peer,_=listener.accept(); peer.setblocking(False); accepted=True
    print('server accepted',flush=True); assert four(peer)==b'PING'; print('server recv=4',flush=True)
    four(peer,b'PONG'); print('server send=4',flush=True); assert four(client)==b'PONG'; print('client recv=4',flush=True); ok=True
except Exception as error: print('error={} {}'.format(type(error).__name__,error),flush=True)
finally:
    if peer: peer.close()
    client.close(); listener.close()
print('PROBE RESULT: {} accepted={}'.format('ok' if ok else 'blocked-or-error',accepted),flush=True)
sys.exit(0 if ok and accepted else 1)
