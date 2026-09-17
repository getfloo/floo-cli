"""Local authenticated TLS-inspecting proxy; never connects to external hosts."""
import base64
import hashlib
import http.server
import json
import pathlib
import ssl
import subprocess
import sys

directory, asset, signature = sys.argv[1:]
directory = pathlib.Path(directory)


def openssl(*args):
    subprocess.run(["openssl", *args], cwd=directory, check=True, capture_output=True)


openssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256",
        "-nodes", "-keyout", "ca.key", "-out", "ca.pem", "-days", "2", "-subj", "/CN=Test CA")
openssl("req", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes",
        "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=releases.test")
(directory / "extensions").write_text("subjectAltName=DNS:releases.test\nbasicConstraints=CA:FALSE\n")
openssl("x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key",
        "-CAcreateserial", "-out", "server.pem", "-days", "2", "-extfile", "extensions")
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(directory / "server.pem", directory / "server.key")
binary = b"fake-binary-content"
bodies = {"/binary": binary, "/checksum": hashlib.sha256(binary).hexdigest().encode(),
          "/signature": base64.b64decode(signature)}
bodies["/releases/latest"] = json.dumps({"tag_name": "v9999.0.0", "assets": [
    {"name": asset + suffix, "browser_download_url": "https://releases.test" + path}
    for suffix, path in [("", "/binary"), (".sha256", "/checksum"), (".sig", "/signature")]
]}).encode()


class Proxy(http.server.BaseHTTPRequestHandler):
    def do_CONNECT(self):
        assert self.headers["Proxy-Authorization"] == "Basic " + base64.b64encode(b"user:proxy-secret").decode()
        self.send_response(200)
        self.end_headers()
        try:
            self.connection = context.wrap_socket(self.connection, server_side=True)
        except ssl.SSLError:
            return  # Expected for the untrusted-CA and hostname-negative cases.
        self.rfile = self.connection.makefile("rb")
        self.wfile = self.connection.makefile("wb")
        self.handle_one_request()
        self.wfile.flush()

    def do_GET(self):
        body = bodies[self.path]
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Proxy)
print(server.server_port, flush=True)
server.serve_forever()
