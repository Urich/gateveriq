"""Bounded local security probes. Build first: cargo build --locked.

Run: python3 tests/security_audit.py
Uses only temporary state and listeners bound to 127.0.0.1. Finding probes
describe the current behavior; they are not assertions that it is secure.
No external target or integration credentials are accepted.
"""
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/gateveriq"
ORIGIN = "http://127.0.0.1:8090"
OTHER_ORIGIN = "http://127.0.0.1:8091"
RESULTS = {"checks": [], "observations": {}}
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def check(name, condition):
    if not condition:
        raise AssertionError(name)
    RESULTS["checks"].append(name)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def request(port, path, body=None, integration=None, origin=None,
            raw=None, content_type="application/json", method="POST"):
    headers = {"Content-Type": content_type}
    if integration:
        headers["Authorization"] = "Bearer " + integration["integration_secret"]
    if origin:
        headers["Origin"] = origin
    data = raw if raw is not None else json.dumps(body).encode()
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data,
                                 headers, method=method)
    try:
        response = HTTP.open(req, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        data = response.read()
        try:
            value = json.loads(data)
        except (ValueError, UnicodeDecodeError):
            value = None
        return response.status, value


def proof(challenge):
    decode = lambda value: base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))
    prefix = (b"gateveriq:pow:v1" + decode(challenge["challenge_id"])
              + decode(challenge["salt"]))
    difficulty = challenge["difficulty_bits"]
    for counter in range(10_000_000):
        nonce = counter.to_bytes(8, "little")
        digest = hashlib.sha256(prefix + nonce).digest()
        if int.from_bytes(digest, "big") >> (256 - difficulty) == 0:
            return {"challenge_id": challenge["challenge_id"],
                    "nonce": base64.urlsafe_b64encode(nonce).rstrip(b"=").decode()}
    raise RuntimeError("Local proof budget exceeded")


def synthetic_signals(solution):
    return {**solution,
            "browser": {"webdriver": False, "cookies_enabled": True,
                        "languages_count": 1},
            "behavior": {"challenge_id": solution["challenge_id"],
                         "elapsed_ms": 1000, "activation_ms": 500,
                         "trusted_activations": 1, "pointer_events": 1,
                         "keyboard_events": 0, "focus_changes": 0}}


def fixture_position(database, visual):
    # Privileged protocol-test fixture, never evidence of bot resistance.
    with sqlite3.connect(database) as db:
        return db.execute('SELECT answer FROM visual_tasks WHERE task_id=?', [visual['task_id']]).fetchone()[0]


def run():
    with tempfile.TemporaryDirectory(prefix="gateveriq-security-") as directory:
        def cli(*arguments):
            return subprocess.check_output([str(BINARY), *arguments], cwd=directory, text=True)

        cli("init")
        public, internal = free_port(), free_port()
        while public == internal:
            internal = free_port()
        config = Path(directory) / "config.toml"
        config.write_text(config.read_text()
                          .replace("127.0.0.1:9080", f"127.0.0.1:{public}")
                          .replace("127.0.0.1:9081", f"127.0.0.1:{internal}")
                          .replace("difficulty_bits = 18", "difficulty_bits = 8"))
        legacy = json.loads(cli("integration-create", "--name", "audit-legacy", "--actions", "login"))
        website = json.loads(cli("integration-create", "--name", "audit-web", "--actions", "login,register",
                                 "--origins", ORIGIN + "," + OTHER_ORIGIN))
        cli("integration-visual-policy", "--id", website["integration_id"],
            "--action", "register", "--visual", "always")
        process = subprocess.Popen([str(BINARY), "serve"], cwd=directory,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError("Local service exited during startup")
                try:
                    if request(public, "/health", method="GET")[0] == 200:
                        break
                except OSError:
                    time.sleep(.05)
            else:
                raise RuntimeError("Local service did not start")

            def context(integration, action):
                status, result = request(internal, "/internal/v1/contexts",
                                         {"integration_id": integration["integration_id"], "action": action}, integration)
                check("issue_context_" + action, status == 200)
                return result["context_id"]

            def web_attempt(action="login", account="audit-account"):
                payload = {"integration_id": website["integration_id"], "action": action,
                           "context_id": context(website, action), "account_id": account,
                           "origin": ORIGIN}
                status, result = request(internal, "/internal/v1/web/attempts", payload, website)
                check("prepare_" + action, status == 200)
                return payload, result

            def consume(payload, token, request_id, integration=website):
                body = {key: payload[key] for key in ("integration_id", "action", "context_id")}
                body.update(integration_id=integration["integration_id"],
                            verification_token=token, request_id=request_id)
                return request(internal, "/internal/v1/verifications/consume", body, integration)

            legacy_body = {"integration_id": legacy["integration_id"], "action": "login", "context_id": context(legacy, "login")}
            check("public_hides_internal", request(public, "/internal/v1/challenges", legacy_body)[0] == 404)
            check("internal_hides_public", request(internal, "/public/v1/solutions", {})[0] == 404)
            check("authentication_required", request(internal, "/internal/v1/challenges", legacy_body)[0] == 401)
            check("integration_secret_isolation", request(internal, "/internal/v1/challenges", legacy_body, website)[0] == 401)
            check("action_allowlist", request(internal, "/internal/v1/contexts",
                  {"integration_id": legacy["integration_id"], "action": "admin"}, legacy)[0] == 403)
            check("sql_metacharacters_rejected", request(internal, "/internal/v1/challenges",
                  {**legacy_body, "action": "login' OR 1=1--"}, legacy)[0] == 400)
            check("unknown_fields_rejected", request(public, "/public/v1/solutions",
                  {"challenge_id": "bad", "nonce": "bad", "difficulty_bits": 0})[0] == 422)
            check("malformed_json_rejected", request(public, "/public/v1/solutions", raw=b"{")[0] == 400)
            check("body_limit", request(public, "/public/v1/solutions", raw=b" " * 4097)[0] == 413)
            check("content_type_enforced", request(public, "/public/v1/solutions", {}, content_type="text/plain")[0] == 415)
            check("browser_cannot_report_outcome", request(public, "/internal/v1/web/outcomes", {})[0] == 404)

            payload, attempt = web_attempt()
            solution = synthetic_signals(proof(attempt["challenge"]))
            for name, origin, endpoint in [
                ("missing_origin", None, "/public/v1/web/solutions"),
                ("foreign_origin", "https://attacker.invalid", "/public/v1/web/solutions"),
                ("different_allowed_origin", OTHER_ORIGIN, "/public/v1/web/solutions"),
                ("legacy_route_cannot_issue_website_token", None, "/public/v1/solutions"),
            ]:
                status, result = request(public, endpoint, solution, origin=origin)
                check(name, status == 403 and result["error"]["code"] == "ORIGIN_NOT_ALLOWED")
            check("account_binding_immutable", request(internal, "/internal/v1/web/attempts",
                  {**payload, "account_id": "different-account"}, website)[0] == 400)
            status, verified = request(public, "/public/v1/web/solutions", solution, origin=ORIGIN)
            check("synthetic_reports_obtain_pow_token", status == 200 and bool(verified.get("verification_token")))
            RESULTS["observations"]["browserless_pow"] = "Accepted forged browser/behavior signals with valid PoW; documented limitation"
            token = verified["verification_token"]
            check("token_integration_isolation", consume(payload, token, "foreign", legacy)[0] == 400)
            check("token_context_binding", consume({**payload, "context_id": secrets.token_urlsafe(32)}, token, "wrong")[0] == 400)
            with ThreadPoolExecutor(max_workers=8) as pool:
                attempts = list(pool.map(lambda i: consume(payload, token, f"concurrent-{i}"), range(8)))
            check("single_concurrent_consumption", sum(status == 200 for status, _ in attempts) == 1)
            winner = next(i for i, (status, _) in enumerate(attempts) if status == 200)
            check("exact_receipt_retry", consume(payload, token, f"concurrent-{winner}")[0] == 200)
            check("token_replay_rejected", consume(payload, token, "new-request")[0] == 400)
            check("consumed_token_not_reissued", request(public, "/public/v1/web/solutions", solution, origin=ORIGIN)[0] == 400)

            solved = 0
            for index in range(5):
                payload, attempt = web_attempt("register", f"visual-account-{index}")
                visual = attempt["visual"]
                solution = {**proof(attempt["challenge"]), "task_id": visual["task_id"],
                            "position": fixture_position(Path(directory) / "gateveriq.sqlite3", visual)}
                if index == 0:
                    check("visual_rejects_foreign_origin", request(public, "/public/v1/web/visual/solutions", solution,
                          origin="https://attacker.invalid")[0] == 403)
                    check("visual_rejects_old_selection_schema", request(public, "/public/v1/web/visual/solutions",
                          {**solution, "selected": [0, 0]}, origin=ORIGIN)[0] == 422)
                    check("visual_rejects_position_overflow", request(public, "/public/v1/web/visual/solutions",
                          {**solution, "position": 241}, origin=ORIGIN)[0] == 400)
                status, verified = request(public, "/public/v1/web/visual/solutions", solution, origin=ORIGIN)
                check("slider_protocol_" + str(index), status == 200 and bool(verified.get("verification_token")))
                status, receipt = consume(payload, verified["verification_token"], f"visual-{index}")
                check("visual_receipt_" + str(index), status == 200 and receipt["verification_method"] == "pow_visual")
                solved += 1
            RESULTS["observations"]["slider_protocol"] = {"solved": solved, "attempts": 5,
                "server_answer_read": True, "browser_used": False,
                "classification": "privileged fixture; computer-vision resistance not tested"}

            payload, attempt = web_attempt("register", "wrong-answers")
            visual = attempt["visual"]
            solution = proof(attempt["challenge"])
            for index in range(3):
                status, result = request(public, "/public/v1/web/visual/solutions",
                                         {**solution, "task_id": visual["task_id"], "position": 0}, origin=ORIGIN)
                if index < 2:
                    replacement = result["visual"]
                    check("wrong_visual_rotates_" + str(index), status == 400
                          and replacement["task_id"] != visual["task_id"]
                          and replacement["expires_at"] == visual["expires_at"]
                          and replacement["max_attempts"] == 2 - index)
                    check("stale_visual_task_rejected_" + str(index), request(public, "/public/v1/web/visual/solutions",
                          {**solution, "task_id": visual["task_id"], "position": 0}, origin=ORIGIN)[0] == 400)
                    visual = replacement
                else:
                    check("visual_budget_exhausted", status == 429 and result["error"]["code"] == "VISUAL_ATTEMPTS_EXHAUSTED")
            check("visual_budget_cannot_reset", request(internal, "/internal/v1/web/attempts", payload, website)[0] == 429)

            # A peer behind the same reverse proxy shares these counters. No PoW
            # is required to spend the source failure budget with known IDs.
            website = json.loads(cli("integration-create", "--name", "audit-history", "--actions", "login",
                                     "--origins", ORIGIN))
            payload, attempt = web_attempt("login", "history-probe")
            invalid = {"challenge_id": attempt["challenge"]["challenge_id"], "nonce": "invalid"}
            for index in range(5):
                check("unproven_history_failure_" + str(index), request(public, "/public/v1/solutions", invalid)[0] == 400)
            status, result = request(public, "/public/v1/web/solutions",
                                     synthetic_signals(proof(attempt["challenge"])), origin=ORIGIN)
            check("shared_peer_history_forces_visual", status == 403 and result["decision"] == "additional_verification_required")
            RESULTS["observations"]["shared_peer_history"] = "Five requests without valid PoW or Origin force visual step-up for the shared peer"

            # Simulate time passing, never wait for expiry or fill a disk.
            database = Path(directory) / "gateveriq.sqlite3"
            with sqlite3.connect(database) as db:
                db.execute("UPDATE challenges SET expires=0,token_expires=0,consumed_at=NULL")
            cli("cleanup")
            with sqlite3.connect(database) as db:
                challenges = db.execute("SELECT COUNT(*) FROM challenges").fetchone()[0]
                contexts = db.execute("SELECT COUNT(*) FROM sqlite_master WHERE name='used_contexts'").fetchone()[0]
            check("cleanup_removes_expired_challenges", challenges == 0)
            check("no_permanent_context_table", contexts == 0)
            for index in range(10):
                status, _ = request(internal, "/internal/v1/challenges",
                                    {**legacy_body, "context_id": context(legacy, "login")}, legacy)
                check("second_cleanup_prepare_" + str(index), status == 200)
            with sqlite3.connect(database) as db:
                db.execute("UPDATE challenges SET expires=0")
            cli("cleanup")
            with sqlite3.connect(database) as db:
                second_contexts = db.execute("SELECT COUNT(*) FROM sqlite_master WHERE name='used_contexts'").fetchone()[0]
                check("second_cleanup_removes_challenges", db.execute("SELECT COUNT(*) FROM challenges").fetchone()[0] == 0)
            check("no_context_tombstones_after_second_cycle", second_contexts == 0)
            RESULTS["observations"]["cleanup"] = {"challenges_after": challenges,
                "permanent_context_tables": contexts,
                "permanent_context_tables_after_second_cycle": second_contexts}
            check("service_remains_healthy", request(public, "/health", method="GET")[0] == 200)
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            process.stderr.close()
    RESULTS["passed"] = len(RESULTS["checks"])
    print(json.dumps(RESULTS, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    run()
