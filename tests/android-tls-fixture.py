"""Local, deliberately untrusted HTTPS endpoint for Android instrumentation tests."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ssl

parser = argparse.ArgumentParser()
parser.add_argument('--cert', required=True)
parser.add_argument('--key', required=True)
parser.add_argument('--port', type=int, default=4448)
args = parser.parse_args()

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *arguments):
        pass

    def do_GET(self):
        body = b'<!doctype html><title>TLS fixture</title><body>TLS_FIXTURE_MUST_NOT_LOAD</body>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(args.cert, args.key)
server = ThreadingHTTPServer(('127.0.0.1', args.port), Handler)
server.socket = context.wrap_socket(server.socket, server_side=True)
server.serve_forever()
