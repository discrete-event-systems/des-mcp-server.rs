-- Declarative Supabase schema for discrete-event-systems client telemetry
-- (dpm-style: this file describes the DESIRED end state; apply with `dpm`
-- from github.com/declarative-migrations — no tracked migration files).
--
-- Org telemetry prefix: des_. Future web/WASM sim visualizers stream client
-- logs DIRECTLY to Supabase via the ingest RPCs below (supabase-js for
-- WASM/TS, supabase_flutter for Dart), modeled on dd-next-1 migrations
-- 029-035 and simplified. RLS: anon/authenticated may INSERT only (via RPC
-- or direct REST fallback); reads require service_role (the MCP server's
-- debugging tools).

-- ---------------------------------------------------------------- tables

create table if not exists public.des_client_log_snapshots (
  id                     uuid primary key default gen_random_uuid(),
  user_id                uuid,
  session_id             varchar(100),
  -- which simulation/model the visualizer was running (org-specific):
  -- e.g. a des-engine examples/*.json registry id or a des_engine demo bin
  sim_id                 varchar(120),
  log_entries_json       text,
  log_entries_count      integer not null default 0,
  log_entries_size_bytes integer not null default 0,
  trigger                jsonb   not null default '{}'::jsonb,
  context                jsonb   not null default '{}'::jsonb,
  trace_id               varchar(150),
  commit_id              varchar(60),
  environment            varchar(30) not null default 'unknown',
  meta                   jsonb   not null default '{}'::jsonb,
  snapshot_taken_at      timestamptz not null default now(),
  created_at             timestamptz not null default now()
);

create index if not exists des_cls_session_id_idx  on public.des_client_log_snapshots (session_id);
create index if not exists des_cls_sim_id_idx      on public.des_client_log_snapshots (sim_id);
create index if not exists des_cls_environment_idx on public.des_client_log_snapshots (environment);
create index if not exists des_cls_taken_at_idx    on public.des_client_log_snapshots (snapshot_taken_at);

create table if not exists public.des_client_log_entries (
  id               uuid primary key default gen_random_uuid(),
  user_id          uuid,
  session_id       varchar(100),
  sim_id           varchar(120),
  trace_id         varchar(150),
  commit_id        varchar(60),
  environment      varchar(30) not null default 'unknown',
  level            varchar(20) not null default 'info',
  message          text not null,
  stack            text,
  url              text,
  source           varchar(50) not null default 'browser',
  category         varchar(50),
  metadata         jsonb not null default '{}'::jsonb,
  client_timestamp timestamptz not null,
  created_at       timestamptz not null default now()
);

create index if not exists des_cle_session_id_idx       on public.des_client_log_entries (session_id);
create index if not exists des_cle_sim_id_idx           on public.des_client_log_entries (sim_id);
create index if not exists des_cle_environment_idx      on public.des_client_log_entries (environment);
create index if not exists des_cle_level_idx            on public.des_client_log_entries (level);
create index if not exists des_cle_client_timestamp_idx on public.des_client_log_entries (client_timestamp);
create index if not exists des_cle_created_at_idx       on public.des_client_log_entries (created_at);

-- ------------------------------------------------------------------- RLS
-- Write-only for clients; service_role (server-side debugging) sees all.

alter table public.des_client_log_snapshots enable row level security;
alter table public.des_client_log_entries   enable row level security;

revoke all on public.des_client_log_snapshots from anon, authenticated;
revoke all on public.des_client_log_entries   from anon, authenticated;
grant insert on public.des_client_log_snapshots to anon, authenticated;
grant insert on public.des_client_log_entries   to anon, authenticated;

drop policy if exists des_anon_insert_snapshots on public.des_client_log_snapshots;
create policy des_anon_insert_snapshots on public.des_client_log_snapshots
  for insert to anon with check (true);
drop policy if exists des_auth_insert_snapshots on public.des_client_log_snapshots;
create policy des_auth_insert_snapshots on public.des_client_log_snapshots
  for insert to authenticated with check (true);
drop policy if exists des_service_all_snapshots on public.des_client_log_snapshots;
create policy des_service_all_snapshots on public.des_client_log_snapshots
  for all to service_role using (true) with check (true);

drop policy if exists des_anon_insert_entries on public.des_client_log_entries;
create policy des_anon_insert_entries on public.des_client_log_entries
  for insert to anon with check (true);
drop policy if exists des_auth_insert_entries on public.des_client_log_entries;
create policy des_auth_insert_entries on public.des_client_log_entries
  for insert to authenticated with check (true);
drop policy if exists des_service_all_entries on public.des_client_log_entries;
create policy des_service_all_entries on public.des_client_log_entries
  for all to service_role using (true) with check (true);

-- ----------------------------------------------------------- ingest RPCs
-- SECURITY DEFINER so RLS-restricted clients can insert through a guarded,
-- validating path. Guards: 600KB snapshot payload cap; <=500 entries/call.

create or replace function public.ingest_des_client_log_snapshot(payload jsonb)
returns jsonb
language plpgsql
security definer
set search_path = public
as $$
declare
  v_entries_json text;
  v_snapshot_id  uuid;
begin
  v_entries_json := coalesce(
    payload->>'log_entries_json',
    (payload->'log_entries')::text
  );
  if v_entries_json is not null and octet_length(v_entries_json) > 600000 then
    return jsonb_build_object(
      'success', false, 'code', 'payload_too_large',
      'error', 'log entries exceed 600KB'
    );
  end if;

  insert into public.des_client_log_snapshots (
    user_id, session_id, sim_id, log_entries_json,
    log_entries_count, log_entries_size_bytes,
    trigger, context, trace_id, commit_id, environment, meta,
    snapshot_taken_at
  ) values (
    nullif(payload->>'user_id', '')::uuid,
    nullif(payload->>'session_id', ''),
    nullif(payload->>'sim_id', ''),
    v_entries_json,
    coalesce((payload->>'log_entries_count')::integer,
             jsonb_array_length(coalesce(payload->'log_entries', '[]'::jsonb))),
    coalesce((payload->>'log_entries_size_bytes')::integer,
             coalesce(octet_length(v_entries_json), 0)),
    coalesce(payload->'trigger', '{}'::jsonb),
    coalesce(payload->'context', '{}'::jsonb),
    nullif(payload->>'trace_id', ''),
    nullif(payload->>'commit_id', ''),
    coalesce(nullif(payload->>'environment', ''), 'unknown'),
    coalesce(payload->'meta', '{}'::jsonb)
      || jsonb_build_object('ingestionTransport', 'supabase-rpc', 'rpcVersion', 1),
    coalesce((payload->>'snapshot_taken_at')::timestamptz, now())
  )
  returning id into v_snapshot_id;

  return jsonb_build_object('success', true, 'snapshot_id', v_snapshot_id);
exception when others then
  return jsonb_build_object('success', false, 'code', 'ingest_failed',
                            'error', sqlerrm);
end;
$$;

create or replace function public.ingest_des_client_log_entries(entries jsonb)
returns jsonb
language plpgsql
security definer
set search_path = public
as $$
declare
  v_count integer;
begin
  if jsonb_typeof(entries) <> 'array' then
    return jsonb_build_object('success', false, 'code', 'bad_request',
                              'error', 'entries must be a JSON array');
  end if;
  if jsonb_array_length(entries) > 500 then
    return jsonb_build_object('success', false, 'code', 'too_many_entries',
                              'error', 'max 500 entries per call');
  end if;

  insert into public.des_client_log_entries (
    user_id, session_id, sim_id, trace_id, commit_id, environment,
    level, message, stack, url, source, category, metadata, client_timestamp
  )
  select
    nullif(e->>'user_id', '')::uuid,
    nullif(e->>'session_id', ''),
    nullif(e->>'sim_id', ''),
    nullif(e->>'trace_id', ''),
    nullif(e->>'commit_id', ''),
    coalesce(nullif(e->>'environment', ''), 'unknown'),
    coalesce(nullif(e->>'level', ''), 'info'),
    coalesce(nullif(e->>'message', ''), '[empty message]'),
    e->>'stack',
    e->>'url',
    coalesce(nullif(e->>'source', ''), 'browser'),
    nullif(e->>'category', ''),
    coalesce(e->'metadata', '{}'::jsonb),
    coalesce((e->>'client_timestamp')::timestamptz, now())
  from jsonb_array_elements(entries) as e;

  get diagnostics v_count = row_count;
  return jsonb_build_object('success', true, 'inserted', v_count);
exception when others then
  return jsonb_build_object('success', false, 'code', 'ingest_failed',
                            'error', sqlerrm);
end;
$$;

grant execute on function public.ingest_des_client_log_snapshot(jsonb)
  to anon, authenticated, service_role;
grant execute on function public.ingest_des_client_log_entries(jsonb)
  to anon, authenticated, service_role;

-- ---------------------------------------------------- realtime publication
-- So debugging UIs can watch logs arrive live over the Realtime websocket.

do $$
begin
  begin
    alter publication supabase_realtime add table public.des_client_log_snapshots;
  exception
    when duplicate_object then null;
    when undefined_object then
      create publication supabase_realtime
        for table public.des_client_log_snapshots;
  end;
  begin
    alter publication supabase_realtime add table public.des_client_log_entries;
  exception
    when duplicate_object then null;
  end;
end;
$$;
