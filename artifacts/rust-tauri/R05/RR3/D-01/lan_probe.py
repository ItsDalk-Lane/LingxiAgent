# 最小对照：Apple 签名解释器，绑定和交换与 C 探针一致。
import os
import socket
import sys
import threading

listener = socket.socket()
listener.bind(('0.0.0.0', 0))
listener.listen(4)
listener.settimeout(6)
port = listener.getsockname()[1]
print('pid={} bind=0.0.0.0:{} connect={}:{}'.format(os.getpid(), port, sys.argv[1], port), flush=True)
accepted = []
def serve():
    try:
        peer, _ = listener.accept()
        accepted.append(True)
        print('server accepted', flush=True)
        peer.settimeout(4)
        data = peer.recv(4)
        print('server recv={}'.format(len(data)), flush=True)
        if data == b'PING':
            peer.sendall(b'PONG')
        peer.close()
    except Exception as error:
        print('server error={}'.format(error), flush=True)
thread = threading.Thread(target=serve)
thread.start()
client = socket.socket()
client.settimeout(4)
ok = False
try:
    client.connect((sys.argv[1], port))
    print('client connected', flush=True)
    client.sendall(b'PING')
    print('client send=4', flush=True)
    response = client.recv(4)
    print('client recv={}'.format(len(response)), flush=True)
    ok = response == b'PONG'
except Exception as error:
    print('client error={}'.format(error), flush=True)
finally:
    client.close()
    thread.join()
    listener.close()
print('PROBE RESULT: {} accepted={}'.format('ok' if ok else 'blocked', bool(accepted)), flush=True)
sys.exit(0 if ok and accepted else 1)
