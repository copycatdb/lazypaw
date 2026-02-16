import { useEffect, useState } from 'react'
import { lp, type Game, type Player } from './api'

interface Props {
  game: Game
  player: Player
  onPlayAgain: () => void
}

export default function GameOver({ game, player, onPlayAgain }: Props) {
  const [players, setPlayers] = useState<Player[]>([])
  const [showConfetti, setShowConfetti] = useState(false)

  useEffect(() => {
    (async () => {
      const { data } = await lp.from<Player>('players')
        .select('*')
        .eq('game_id', game.id)
        .order('score', { ascending: false })
      if (data) {
        setPlayers(data)
        // Confetti if player is top 3
        const idx = data.findIndex(p => p.id === player.id)
        if (idx < 3) {
          setShowConfetti(true)
          fireConfetti()
        }
      }
    })()
  }, [game.id])

  const fireConfetti = async () => {
    try {
      const confetti = (await import('canvas-confetti')).default
      const duration = 3000
      const end = Date.now() + duration
      const colors = ['#a855f7', '#ec4899', '#eab308', '#22c55e', '#3b82f6']

      const frame = () => {
        confetti({
          particleCount: 3,
          angle: 60,
          spread: 55,
          origin: { x: 0, y: 0.7 },
          colors,
        })
        confetti({
          particleCount: 3,
          angle: 120,
          spread: 55,
          origin: { x: 1, y: 0.7 },
          colors,
        })
        if (Date.now() < end) requestAnimationFrame(frame)
      }
      frame()
    } catch {}
  }

  const medals = ['🥇', '🥈', '🥉']
  const myRank = players.findIndex(p => p.id === player.id) + 1

  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <div className="max-w-lg w-full space-y-8 text-center">
        {/* Winner announcement */}
        <div className="animate-bounce-in">
          <div className="text-8xl mb-4">🏆</div>
          <h1 className="text-4xl font-black">Game Over!</h1>
          {myRank === 1 && (
            <p className="text-2xl text-yellow-400 font-bold mt-2 animate-pulse">
              🎉 You won! 🎉
            </p>
          )}
          {myRank > 1 && myRank <= 3 && (
            <p className="text-xl text-gray-300 mt-2">
              Nice! You placed #{myRank}!
            </p>
          )}
          {myRank > 3 && (
            <p className="text-xl text-gray-400 mt-2">
              You placed #{myRank} — better luck next time!
            </p>
          )}
        </div>

        {/* Leaderboard */}
        <div className="space-y-3">
          {players.map((p, i) => (
            <div
              key={p.id}
              className={`flex items-center gap-4 rounded-xl p-4 animate-slide-up ${
                p.id === player.id
                  ? 'bg-purple-500/20 border border-purple-500/50'
                  : 'bg-gray-800/50'
              }`}
              style={{ animationDelay: `${i * 150}ms` }}
            >
              <span className="text-3xl w-10 text-center">
                {i < 3 ? medals[i] : `#${i + 1}`}
              </span>
              <span className="text-xl font-bold flex-1 text-left">{p.name}</span>
              <span className="text-2xl font-mono font-bold text-yellow-400">
                {p.score}
              </span>
            </div>
          ))}
        </div>

        {/* Play again */}
        <button
          onClick={onPlayAgain}
          className="w-full py-4 rounded-xl bg-gradient-to-r from-purple-600 to-pink-600
            text-white text-lg font-bold hover:from-purple-500 hover:to-pink-500
            transition-all duration-200 hover:scale-[1.02] active:scale-[0.98]"
        >
          🔄 Play Again
        </button>

        {/* Tech footer */}
        <div className="text-gray-600 text-xs space-y-1">
          <p>Powered by <span className="text-purple-400">lazypaw</span> — PostgREST for SQL Server</p>
          <p>Zero backend code • SQL Server + Change Tracking • React</p>
          <p className="text-gray-700">github.com/copycatdb/lazypaw</p>
        </div>
      </div>
    </div>
  )
}
