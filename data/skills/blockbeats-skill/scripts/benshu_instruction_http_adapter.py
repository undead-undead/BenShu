#!/usr/bin/env python3
import html, json, os, re, sys, urllib.parse, urllib.request

def args():
    if len(sys.argv) < 2:
        return {}
    try:
        value = json.loads(sys.argv[1])
        return value if isinstance(value, dict) else {"query": str(value)}
    except Exception:
        return {"query": sys.argv[1]}

def manual():
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(os.path.dirname(here), "SKILL.md"), "r", encoding="utf-8") as f:
        return f.read()

def clean(value):
    text = html.unescape(str(value or ""))
    text = re.sub(r"<[^>]+>", " ", text)
    return re.sub(r"\s+", " ", text).strip()

def base_url(doc):
    m = re.search(r"Base URL\*\*:\s*`([^`]+)`", doc)
    if m: return m.group(1).rstrip("/")
    urls = re.findall(r"https://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+", doc)
    if not urls: raise RuntimeError("No HTTPS API base URL found in SKILL.md")
    p = urllib.parse.urlparse(urls[0])
    return f"{p.scheme}://{p.netloc}"

def env_name(doc):
    m = re.search(r"primaryEnv:\s*([A-Za-z_][A-Za-z0-9_]*)", doc)
    if m: return m.group(1)
    m = re.search(r"\$([A-Z][A-Z0-9_]*_API_KEY|[A-Z][A-Z0-9_]*API_KEY)", doc)
    return m.group(1) if m else ""

def header_name(doc):
    if "api-key" in doc: return "api-key"
    if "X-API-Key" in doc: return "X-API-Key"
    return "Authorization"

def requests(a):
    lang, size, page = a.get("lang") or "en", int(a.get("size") or 10), int(a.get("page") or 1)
    query = a.get("query") or a.get("name") or ""
    if a.get("endpoint_path"):
        return [{"path": a["endpoint_path"], "params": a.get("params") or {}}]
    action = (a.get("action") or "latest_newsflash").lower()
    mapping = {
        "latest": "/v1/newsflash", "latest_news": "/v1/newsflash", "latest_newsflash": "/v1/newsflash",
        "newsflash": "/v1/newsflash", "important": "/v1/newsflash/important",
        "important_newsflash": "/v1/newsflash/important", "ai": "/v1/newsflash/ai",
        "ai_news": "/v1/newsflash/ai", "onchain": "/v1/newsflash/onchain",
        "financing": "/v1/newsflash/financing", "prediction": "/v1/newsflash/prediction",
        "latest_articles": "/v1/article", "article": "/v1/article", "articles": "/v1/article",
    }
    if action == "search":
        return [{"path": "/v1/search", "params": {"name": query, "size": size, "lang": lang}}]
    if action in mapping:
        return [{"path": mapping[action], "params": {"page": page, "size": size, "lang": lang}}]
    if query:
        return [{"path": "/v1/search", "params": {"name": query, "size": size, "lang": lang}}]
    raise RuntimeError("No endpoint_path or supported action provided")

def call(base, path, params, header, key):
    if not path.startswith("/"): path = "/" + path
    url = base + path + (("?" + urllib.parse.urlencode(params)) if params else "")
    headers = {"Accept": "application/json", "User-Agent": "BenShu skill adapter/1.0"}
    if key: headers[header] = f"Bearer {key}" if header == "Authorization" else key
    req = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw_bytes = resp.read(1024 * 1024 + 1)
        if len(raw_bytes) > 1024 * 1024:
            raise RuntimeError("response exceeded 1MB safety limit")
        raw = raw_bytes.decode(resp.headers.get_content_charset() or "utf-8", "replace")
    payload = json.loads(raw)
    data = payload.get("data", payload) if isinstance(payload, dict) else payload
    return {"url": url, "payload": payload, "compact": compact(data)}

def item(x):
    if not isinstance(x, dict): return x
    return {"title": clean(x.get("title") or x.get("name") or ""), "summary": clean(x.get("abstract") or x.get("description") or x.get("content") or ""), "time": x.get("time_cn") or x.get("create_time") or x.get("time") or "", "url": x.get("url") or x.get("link") or ""}

def compact(data):
    if isinstance(data, dict):
        rows = data.get("list") or data.get("data") or data.get("items") or data.get("rows")
        if isinstance(rows, list):
            data = dict(data)
            data["items"] = [item(v) for v in rows[:10]]
    elif isinstance(data, list):
        data = [item(v) for v in data[:10]]
    return data

def main():
    a, doc = args(), manual()
    base, env, header = base_url(doc), env_name(doc), header_name(doc)
    key = os.environ.get(env, "") if env else ""
    out, errors = [], []
    for r in requests(a):
        try: out.append(call(base, r["path"], r.get("params") or {}, header, key))
        except Exception as e: errors.append({"path": r.get("path"), "error": f"{type(e).__name__}: {e}"})
    print(json.dumps({"ok": bool(out) and not errors, "base_url": base, "used_env": env if key else "", "results": out, "errors": errors}, ensure_ascii=False))

if __name__ == "__main__":
    main()
