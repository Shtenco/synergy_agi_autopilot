from __future__ import annotations
import json, sqlite3, threading, time, uuid
from pathlib import Path


class TaskStore:
    def __init__(self,path:str="autopilot_runtime.db"):
        self.path=path; self._lock=threading.RLock(); Path(path).parent.mkdir(parents=True,exist_ok=True)
        with self._db() as c:
            c.executescript('''
            create table if not exists tasks(
              id text primary key, capability text not null, params_json text not null,
              status text not null, policy_json text, result_json text, error text,
              approved_by text, created_at real not null, updated_at real not null,
              idempotency_key text unique
            );
            create table if not exists events(
              id integer primary key autoincrement, task_id text, kind text not null,
              data_json text not null, created_at real not null
            );
            ''')
    def _db(self):
        c=sqlite3.connect(self.path,timeout=20); c.row_factory=sqlite3.Row; return c
    def create(self,capability:str,params:dict,idempotency_key:str|None=None)->dict:
        now=time.time(); tid="apt_"+uuid.uuid4().hex
        with self._lock,self._db() as c:
            if idempotency_key:
                row=c.execute("select * from tasks where idempotency_key=?",(idempotency_key,)).fetchone()
                if row:return self._row(row)
            c.execute("insert into tasks(id,capability,params_json,status,created_at,updated_at,idempotency_key) values(?,?,?,?,?,?,?)",(tid,capability,json.dumps(params,ensure_ascii=False),"proposed",now,now,idempotency_key))
            self._event(c,tid,"created",{"capability":capability})
            row=c.execute("select * from tasks where id=?",(tid,)).fetchone(); return self._row(row)
    def get(self,tid:str)->dict:
        with self._db() as c:
            row=c.execute("select * from tasks where id=?",(tid,)).fetchone()
            if not row:raise KeyError(tid)
            return self._row(row)
    def update(self,tid:str,**fields)->dict:
        allowed={"status","policy_json","result_json","error","approved_by"}; fields={k:v for k,v in fields.items() if k in allowed}; fields["updated_at"]=time.time()
        with self._lock,self._db() as c:
            if not c.execute("select 1 from tasks where id=?",(tid,)).fetchone():raise KeyError(tid)
            sql=", ".join(f"{k}=?" for k in fields); c.execute(f"update tasks set {sql} where id=?",(*fields.values(),tid)); self._event(c,tid,"updated",fields)
            return self._row(c.execute("select * from tasks where id=?",(tid,)).fetchone())
    def _event(self,c,tid,kind,data): c.execute("insert into events(task_id,kind,data_json,created_at) values(?,?,?,?)",(tid,kind,json.dumps(data,ensure_ascii=False,default=str),time.time()))
    @staticmethod
    def _row(r):
        d=dict(r)
        for src,dst in [("params_json","params"),("policy_json","policy"),("result_json","result")]:
            raw=d.pop(src,None); d[dst]=json.loads(raw) if raw else None
        return d
