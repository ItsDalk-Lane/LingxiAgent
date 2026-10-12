import hashlib, json, pathlib, sqlite3, stat, sys
# 只读取本轮明确自有的数据；FIFO 只记录类型，禁止打开以免等待。
out = pathlib.Path(sys.argv[1]); roots = [pathlib.Path(p) for p in sys.argv[2:]]
files = []; databases = []
for root in roots:
    for p in sorted(root.rglob('*')):
        mode = p.lstat().st_mode
        if stat.S_ISREG(mode):
            files.append({'path':str(p),'size':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'kind':'regular'})
            if p.name == 'runs.db':
                dest = out / ('sql-' + hashlib.sha256(str(p).encode()).hexdigest()[:18] + '.sql')
                conn = sqlite3.connect(p.as_uri() + '?mode=ro', uri=True)
                conn.execute('BEGIN'); dest.write_text('\n'.join(conn.iterdump())); conn.close()
                databases.append({'actual_db':str(p),'snapshot':str(dest),'readonly':True})
        elif stat.S_ISFIFO(mode):
            files.append({'path':str(p),'kind':'fifo','size':p.stat().st_size,'sha256':None})
        elif stat.S_ISLNK(mode):
            files.append({'path':str(p),'kind':'symlink','followed':False})
(out / 'owned-files-and-sql.json').write_text(json.dumps({'roots':[str(p)for p in roots],'files':files,'databases':databases,'retained_local_only_never_git':True},indent=2))
