import { useState } from 'react'
import { lp, type Game, type Player } from './api'
import Lobby from './Lobby'
import GamePlay from './GamePlay'
import GameOver from './GameOver'

type Screen = 'home' | 'lobby' | 'playing' | 'finished'

export default function App() {
  const [screen, setScreen] = useState<Screen>('home')
  const [game, setGame] = useState<Game | null>(null)
  const [player, setPlayer] = useState<Player | null>(null)
  const [isHost, setIsHost] = useState(false)
  const [joinCode, setJoinCode] = useState('')
  const [playerName, setPlayerName] = useState('')
  const [error, setError] = useState('')

  const createGame = async () => {
    if (!playerName.trim()) { setError('Enter your name!'); return }
    setError('')
    const code = Math.random().toString(36).substring(2, 8).toUpperCase()

    const { data: gameData, error: gameErr } = await lp.from<Game>('games')
      .insert({ code, host_name: playerName.trim(), status: 'waiting', current_question: 0 })

    if (gameErr || !gameData?.[0]) { setError(gameErr?.message || 'Failed to create game'); return }
    const newGame = gameData[0]

    // Seed questions
    const questions = getTrivia(newGame.id)
    await lp.from('questions').insert(questions)

    // Join as player
    const { data: pData } = await lp.from<Player>('players')
      .insert({ game_id: newGame.id, name: playerName.trim(), score: 0 })

    if (pData?.[0]) {
      setGame(newGame)
      setPlayer(pData[0])
      setIsHost(true)
      setScreen('lobby')
    }
  }

  const joinGame = async () => {
    if (!playerName.trim()) { setError('Enter your name!'); return }
    if (!joinCode.trim()) { setError('Enter a game code!'); return }
    setError('')

    const { data: games } = await lp.from<Game>('games')
      .select('*')
      .eq('code', joinCode.trim().toUpperCase())
      .single()

    if (!games) { setError('Game not found!'); return }
    const g = games as unknown as Game
    if (g.status !== 'waiting') { setError('Game already started!'); return }

    const { data: pData } = await lp.from<Player>('players')
      .insert({ game_id: g.id, name: playerName.trim(), score: 0 })

    if (pData?.[0]) {
      setGame(g)
      setPlayer(pData[0])
      setIsHost(false)
      setScreen('lobby')
    }
  }

  if (screen === 'lobby' && game && player) {
    return (
      <Lobby
        game={game}
        player={player}
        isHost={isHost}
        onStart={(g) => { setGame(g); setScreen('playing') }}
        onGameUpdate={setGame}
      />
    )
  }

  if (screen === 'playing' && game && player) {
    return (
      <GamePlay
        game={game}
        player={player}
        isHost={isHost}
        onFinish={(g) => { setGame(g); setScreen('finished') }}
        onGameUpdate={setGame}
      />
    )
  }

  if (screen === 'finished' && game && player) {
    return (
      <GameOver
        game={game}
        player={player}
        onPlayAgain={() => { setScreen('home'); setGame(null); setPlayer(null) }}
      />
    )
  }

  // Home screen
  return (
    <div className="min-h-screen flex items-center justify-center p-4">
      <div className="max-w-md w-full space-y-8">
        {/* Logo */}
        <div className="text-center animate-bounce-in">
          <div className="text-8xl mb-4">🐱</div>
          <h1 className="text-5xl font-black bg-gradient-to-r from-yellow-400 via-pink-500 to-purple-500 bg-clip-text text-transparent">
            CopyCat Trivia
          </h1>
          <p className="text-gray-400 mt-2 text-sm">
            Powered by lazypaw — zero backend code
          </p>
        </div>

        {/* Name input */}
        <div className="space-y-4 animate-slide-up">
          <input
            type="text"
            placeholder="Your name"
            value={playerName}
            onChange={(e) => setPlayerName(e.target.value)}
            className="w-full px-4 py-3 rounded-xl bg-gray-800 border border-gray-700 text-white text-lg
              focus:outline-none focus:border-purple-500 focus:ring-2 focus:ring-purple-500/50"
            maxLength={20}
          />

          {/* Create game */}
          <button
            onClick={createGame}
            className="w-full py-4 rounded-xl bg-gradient-to-r from-purple-600 to-pink-600
              text-white text-lg font-bold hover:from-purple-500 hover:to-pink-500
              transition-all duration-200 hover:scale-[1.02] active:scale-[0.98]"
          >
            🎮 Create Game
          </button>

          {/* Divider */}
          <div className="flex items-center gap-3">
            <div className="flex-1 h-px bg-gray-700" />
            <span className="text-gray-500 text-sm">or join</span>
            <div className="flex-1 h-px bg-gray-700" />
          </div>

          {/* Join game */}
          <div className="flex gap-2">
            <input
              type="text"
              placeholder="Game code"
              value={joinCode}
              onChange={(e) => setJoinCode(e.target.value.toUpperCase())}
              className="flex-1 px-4 py-3 rounded-xl bg-gray-800 border border-gray-700 text-white text-lg
                uppercase tracking-widest text-center
                focus:outline-none focus:border-green-500 focus:ring-2 focus:ring-green-500/50"
              maxLength={6}
            />
            <button
              onClick={joinGame}
              className="px-6 py-3 rounded-xl bg-green-600 text-white font-bold
                hover:bg-green-500 transition-all duration-200"
            >
              Join
            </button>
          </div>

          {error && (
            <p className="text-red-400 text-center text-sm animate-slide-up">{error}</p>
          )}
        </div>

        {/* Footer */}
        <p className="text-center text-gray-600 text-xs">
          Built with lazypaw + React • No backend code • SQL Server + Change Tracking
        </p>
      </div>
    </div>
  )
}

function getTrivia(gameId: number) {
  return [
    { game_id: gameId, text: 'What year was SQL Server first released?', option_a: '1989', option_b: '1993', option_c: '2000', option_d: '1985', correct: 'A', order_num: 1 },
    { game_id: gameId, text: 'Which protocol does SQL Server use?', option_a: 'HTTP', option_b: 'TDS', option_c: 'ODBC', option_d: 'gRPC', correct: 'B', order_num: 2 },
    { game_id: gameId, text: 'Max size of NVARCHAR(MAX)?', option_a: '2 GB', option_b: '4 GB', option_c: '8 GB', option_d: '1 GB', correct: 'A', order_num: 3 },
    { game_id: gameId, text: 'Which feature provides row-level security?', option_a: 'Always Encrypted', option_b: 'Data Masking', option_c: 'Security Policies', option_d: 'TDE', correct: 'C', order_num: 4 },
    { game_id: gameId, text: 'What language is tabby (TDS library) written in?', option_a: 'Go', option_b: 'C++', option_c: 'Rust', option_d: 'Python', correct: 'C', order_num: 5 },
    { game_id: gameId, text: 'Which CopyCat tool is a REST API server?', option_a: 'prowl', option_b: 'meow', option_c: 'lazypaw', option_d: 'whiskers', correct: 'C', order_num: 6 },
    { game_id: gameId, text: 'What does PostgREST do?', option_a: 'Backup Postgres', option_b: 'REST API from schema', option_c: 'Replication', option_d: 'Monitoring', correct: 'B', order_num: 7 },
    { game_id: gameId, text: 'Default port for SQL Server?', option_a: '3306', option_b: '5432', option_c: '1433', option_d: '27017', correct: 'C', order_num: 8 },
  ]
}
