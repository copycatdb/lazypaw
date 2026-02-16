import { useEffect, useState } from 'react'
import { lp, type Game, type Player } from './api'

interface Props {
  game: Game
  player: Player
  isHost: boolean
  onStart: (game: Game) => void
  onGameUpdate: (game: Game) => void
}

export default function Lobby({ game, player, isHost, onStart, onGameUpdate }: Props) {
  const [players, setPlayers] = useState<Player[]>([player])

  // Initial load + realtime subscriptions
  useEffect(() => {
    // Load existing players
    ;(async () => {
      const { data } = await lp.from<Player>('players')
        .select('*')
        .eq('game_id', game.id)
        .order('joined_at', { ascending: true })
      if (data) setPlayers(data)
    })()

    // Subscribe to new players joining
    const playerChannel = lp.channel('players')
      .on('INSERT', (payload) => {
        const newPlayer = payload.record as Player
        if (newPlayer.game_id === game.id) {
          setPlayers(prev => {
            if (prev.find(p => p.id === newPlayer.id)) return prev
            return [...prev, newPlayer]
          })
        }
      })
      .subscribe()

    // Subscribe to game status changes (for non-hosts to detect start)
    const gameChannel = lp.channel('games')
      .on('UPDATE', (payload) => {
        const g = payload.record as unknown as Game
        if (g.id === game.id) {
          onGameUpdate(g)
          if (g.status === 'playing') onStart(g)
        }
      })
      .subscribe()

    return () => {
      playerChannel.unsubscribe()
      gameChannel.unsubscribe()
    }
  }, [game.id])

  const startGame = async () => {
    const { data } = await lp.from<Game>('games')
      .select('*')
      .eq('id', game.id)
      .update({ status: 'playing', current_question: 1 })
    if (data?.[0]) onStart(data[0])
  }

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <div className="max-w-lg w-full space-y-8 text-center">
        <div className="animate-bounce-in">
          <div className="text-6xl mb-2">🐱</div>
          <h2 className="text-3xl font-bold">Waiting for players...</h2>
        </div>

        {/* Game code */}
        <div className="bg-gray-800/50 rounded-2xl p-6 border border-gray-700">
          <p className="text-gray-400 text-sm mb-2">Share this code</p>
          <p className="text-5xl font-mono font-black tracking-[0.3em] text-yellow-400">
            {game.code}
          </p>
        </div>

        {/* Player list */}
        <div className="space-y-3">
          <h3 className="text-lg text-gray-400">
            Players ({players.length})
          </h3>
          {players.map((p, i) => (
            <div
              key={p.id}
              className="flex items-center gap-3 bg-gray-800/30 rounded-xl p-3 animate-slide-up"
              style={{ animationDelay: `${i * 100}ms` }}
            >
              <span className="text-2xl">
                {['😎', '🤓', '🧠', '🎯', '⚡', '🔥', '💎', '🌟'][i % 8]}
              </span>
              <span className="text-lg font-semibold">{p.name}</span>
              {p.id === player.id && (
                <span className="text-xs bg-purple-500/30 text-purple-300 px-2 py-1 rounded-full ml-auto">
                  You
                </span>
              )}
              {game.host_name === p.name && (
                <span className="text-xs bg-yellow-500/30 text-yellow-300 px-2 py-1 rounded-full">
                  Host
                </span>
              )}
            </div>
          ))}
        </div>

        {/* Start button (host only) */}
        {isHost && players.length >= 1 && (
          <button
            onClick={startGame}
            className="w-full py-4 rounded-xl bg-gradient-to-r from-green-600 to-emerald-600
              text-white text-xl font-bold hover:from-green-500 hover:to-emerald-500
              transition-all duration-200 hover:scale-[1.02] active:scale-[0.98]
              animate-slide-up"
          >
            🚀 Start Game!
          </button>
        )}

        {!isHost && (
          <p className="text-gray-500 animate-pulse">
            Waiting for host to start...
          </p>
        )}
      </div>
    </div>
  )
}
