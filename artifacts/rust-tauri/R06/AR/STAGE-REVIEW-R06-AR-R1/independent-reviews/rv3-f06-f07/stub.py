import http.server, json, sys

class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): 
        sys.stderr.write("STUB-HIT %s\n" % (a,))
    def do_POST(self):
        n = int(self.headers.get('content-length', 0))
        body = self.rfile.read(n)
        with open('/tmp/rv3-review/stub-hits.jsonl', 'ab') as f:
            f.write(body + b'\n')
        # respond with a write tool call on first hit, final text after
        hits = sum(1 for _ in open('/tmp/rv3-review/stub-hits.jsonl', 'rb'))
        if hits == 1:
            delta = json.dumps({"id":"c1","model":"stub","choices":[{"index":0,"finish_reason":None,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call-w","type":"function","function":{"name":"write","arguments":"{\"path\":\"notes/a.md\",\"content\":\"灵犀评测样本\"}"}}]}}]})
            fin = json.dumps({"id":"c1","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
            usage = {"prompt_tokens":30,"completion_tokens":11}
        else:
            delta = json.dumps({"id":"c2","model":"stub","choices":[{"index":0,"finish_reason":None,"delta":{"role":"assistant","content":"完成。"}}]})
            fin = json.dumps({"id":"c2","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})
            usage = {"prompt_tokens":40,"completion_tokens":17}
        sse = f"data: {delta}\n\ndata: {fin}\n\ndata: [DONE]\n\n"
        self.send_response(200)
        self.send_header('content-type','text/event-stream')
        self.send_header('content-length', str(len(sse)))
        self.end_headers()
        self.wfile.write(sse.encode())

http.server.ThreadingHTTPServer(('127.0.0.1', 18099), H).serve_forever()
