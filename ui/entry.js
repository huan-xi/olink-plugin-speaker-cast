import { createElement, useCallback, useEffect, useState } from 'react'
import { usePluginContext } from '@olink/plugin-sdk'

/** 音箱投送管理页（经 Host proxy 调插件 API） */
function HomePage() {
  const { pluginFetch, toast } = usePluginContext()
  const [status, setStatus] = useState(null)
  const [accounts, setAccounts] = useState([])
  const [speakers, setSpeakers] = useState([])
  const [err, setErr] = useState('')

  const reload = useCallback(async () => {
    try {
      setErr('')
      const st = await pluginFetch('/status')
      setStatus(st?.data ?? st)
      const acc = await pluginFetch('/accounts')
      setAccounts(acc?.data ?? acc ?? [])
      const sp = await pluginFetch('/speakers')
      setSpeakers(sp?.data ?? sp ?? [])
    } catch (e) {
      const msg = String(e?.message ?? e)
      setErr(msg)
      toast(msg)
    }
  }, [pluginFetch, toast])

  useEffect(() => {
    reload()
  }, [reload])

  const toggleAirplay = async (did, enabled) => {
    try {
      await pluginFetch(enabled ? '/airplay/disable' : '/airplay/enable', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ did }),
      })
      await reload()
    } catch (e) {
      toast(String(e?.message ?? e))
    }
  }

  return createElement(
    'div',
    { className: 'plg-speaker_cast-page' },
    createElement('h1', null, '音箱投送 AirPlay'),
    err ? createElement('p', { className: 'err' }, err) : null,
    status
      ? createElement(
          'p',
          { className: 'meta' },
          `MiNA ready: ${status.mina_ready ? 'yes' : 'no'} · accounts: ${status.accounts_count ?? 0} · bridge: ${status.bridge_port ?? 0}`,
        )
      : createElement('p', null, '加载中…'),
    createElement('h2', null, '账号'),
    createElement(
      'ul',
      null,
      ...(accounts || []).map((a) =>
        createElement(
          'li',
          { key: a.user_id },
          `${a.nickname || a.user_id} · ${a.ready ? 'ready' : 'not ready'}`,
        ),
      ),
    ),
    createElement('h2', null, '已接入音箱'),
    createElement(
      'ul',
      null,
      ...(speakers || []).map((s) =>
        createElement(
          'li',
          { key: s.did },
          `${s.name || s.did} · AirPlay ${s.airplay_enabled ? 'on' : 'off'} `,
          createElement(
            'button',
            { type: 'button', onClick: () => toggleAirplay(s.did, s.airplay_enabled) },
            s.airplay_enabled ? '关闭 AirPlay' : '开启 AirPlay',
          ),
        ),
      ),
    ),
    createElement('button', { type: 'button', onClick: reload }, '刷新'),
  )
}

export const routes = [{ path: '', element: createElement(HomePage) }]
