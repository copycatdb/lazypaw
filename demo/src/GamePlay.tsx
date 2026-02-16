import { useEffect, useState, useCallback } from 'react'
import { lp, type Game, type Player, type Question, type Answer } from './api'

interface Props {
  game: Game
  player: Player
  isHost: boolean
  onFinish: (game: Game) => void
  onGameUpdate: (game: Game) => void
}

export default function GamePlay({ game, player, isHost, onFinish, onGameUpdate }: Props) {
  const [questions, setQuestions] = useState<Question[]>([])
  const [currentQ, setCurrentQ] = useState<Question | null>(null)
  const [questionNum, setQuestionNum] = useState(game.current_question || 1)
  const [selected, setSelected] = useState<string | null>(null)
  const [revealed, setRevealed] = useState(false)
  const [score, setScore] = useState(0)
  const [answerCount, setAnswerCount] = useState(0)
  const [timer, setTimer] = useState(15)
  const [players, setPlayers] = useState<Player[]>([])

  // Load questions once
  useEffect(() => {
    (async () => {
      const { data } = await lp.from<Question>('questions')
        .select('*')
        .eq('game_id', game.id)
        .order('order_num', { ascending: true })
      if (data) {
        setQuestions(data)
        const q = data.find(q => q.order_num === questionNum)
        if (q) setCurrentQ(q)
      }

      // Load initial players/scores
      const { data: ps } = await lp.from<Player>('players')
        .select('*').eq('game_id', game.id).order('score', { ascending: false })
      if (ps) setPlayers(ps)
    })()
  }, [game.id])

  // Realtime: game updates (question changes, game finish)
  useEffect(() => {
    const gameChannel = lp.channel('games')
      .on('UPDATE', (payload) => {
        const g = payload.record as unknown as Game
        if (g.id !== game.id) return

        if (g.status === 'finished') { onFinish(g); return }

        onGameUpdate(g)
        if (g.current_question !== questionNum) {
          setQuestionNum(g.current_question)
          const q = questions.find(q => q.order_num === g.current_question)
          if (q) {
            setCurrentQ(q)
            setSelected(null)
            setRevealed(false)
            setTimer(15)
            setAnswerCount(0)
          }
        }
      })
      .subscribe()

    return () => gameChannel.unsubscribe()
  }, [game.id, questionNum, questions])

  // Realtime: answers coming in (live count)
  useEffect(() => {
    const answerChannel = lp.channel('answers')
      .on('INSERT', (payload) => {
        const ans = payload.record as unknown as Answer
        if (currentQ && ans.question_id === currentQ.id) {
          setAnswerCount(prev => prev + 1)
        }
      })
      .subscribe()

    return () => answerChannel.unsubscribe()
  }, [currentQ?.id])

  // Realtime: player score updates
  useEffect(() => {
    const playerChannel = lp.channel('players')
      .on('UPDATE', (payload) => {
        const updated = payload.record as unknown as Player
        if (updated.game_id !== game.id) return
        setPlayers(prev => {
          const next = prev.map(p => p.id === updated.id ? updated : p)
          return next.sort((a, b) => b.score - a.score)
        })
      })
      .subscribe()

    return () => playerChannel.unsubscribe()
  }, [game.id])

  // Countdown timer
  useEffect(() => {
    if (revealed || !currentQ) return
    if (timer <= 0) {
      setRevealed(true)
      return
    }
    const t = setTimeout(() => setTimer(timer - 1), 1000)
    return () => clearTimeout(t)
  }, [timer, revealed, currentQ])

  const submitAnswer = useCallback(async (choice: string) => {
    if (selected || revealed || !currentQ) return
    setSelected(choice)

    const isCorrect = choice === currentQ.correct
    if (isCorrect) {
      const points = Math.max(100, 100 + timer * 50)
      setScore(s => s + points)
      await lp.from('players')
        .update({ score: score + points })
        .eq('id', player.id)
    }

    await lp.from('answers').insert({
      player_id: player.id,
      question_id: currentQ.id,
      choice,
      is_correct: isCorrect ? 1 : 0,
    })
  }, [selected, revealed, currentQ, timer, score, player.id])

  const nextQuestion = async () => {
    const next = questionNum + 1
    if (next > questions.length) {
      await lp.from('games').update({ status: 'finished', current_question: questionNum }).eq('id', game.id)
      onFinish({ ...game, status: 'finished' })
    } else {
      await lp.from('games').update({ current_question: next }).eq('id', game.id)
      setQuestionNum(next)
      const q = questions.find(q => q.order_num === next)
      if (q) {
        setCurrentQ(q)
        setSelected(null)
        setRevealed(false)
        setTimer(15)
        setAnswerCount(0)
      }
    }
  }

  if (!currentQ) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="text-2xl text-gray-400 animate-pulse">Loading questions...</div>
      </div>
    )
  }

  const options = [
    { key: 'A', text: currentQ.option_a, cls: 'option-a' },
    { key: 'B', text: currentQ.option_b, cls: 'option-b' },
    { key: 'C', text: currentQ.option_c, cls: 'option-c' },
    { key: 'D', text: currentQ.option_d, cls: 'option-d' },
  ]

  return (
    <div className="min-h-screen p-4 flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-2">
          <span className="text-2xl">🐱</span>
          <span className="text-gray-400 text-sm">
            Q{questionNum}/{questions.length}
          </span>
        </div>
        <div className={`text-3xl font-mono font-bold ${timer <= 5 ? 'text-red-400 animate-pulse' : 'text-white'}`}>
          {timer}s
        </div>
        <div className="text-right">
          <div className="text-sm text-gray-400">{player.name}</div>
          <div className={`text-xl font-bold text-yellow-400 ${selected ? 'animate-pulse-score' : ''}`}>
            {score} pts
          </div>
        </div>
      </div>

      {/* Question */}
      <div className="flex-1 flex flex-col justify-center max-w-2xl mx-auto w-full">
        <h2 className="text-2xl md:text-3xl font-bold text-center mb-8 animate-bounce-in">
          {currentQ.text}
        </h2>

        {/* Options grid */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mb-6">
          {options.map((opt) => {
            let cls = `option-btn ${opt.cls}`
            if (selected === opt.key) cls += ' selected'
            if (revealed && opt.key === currentQ.correct) cls += ' correct'
            if (revealed && selected === opt.key && opt.key !== currentQ.correct) {
              cls += ' opacity-50 line-through'
            }

            return (
              <button
                key={opt.key}
                className={cls}
                onClick={() => submitAnswer(opt.key)}
                disabled={!!selected || revealed}
              >
                <span className="font-black mr-2">{opt.key}.</span>
                {opt.text}
              </button>
            )
          })}
        </div>

        {/* Live answer count */}
        <div className="text-center text-gray-400 text-sm mb-4">
          {answerCount} answer{answerCount !== 1 ? 's' : ''} submitted
        </div>

        {/* Reveal / Next buttons (host) */}
        {isHost && !revealed && (
          <button
            onClick={() => setRevealed(true)}
            className="mx-auto px-6 py-3 rounded-xl bg-yellow-600 text-white font-bold
              hover:bg-yellow-500 transition-all"
          >
            👁️ Reveal Answer
          </button>
        )}

        {isHost && revealed && (
          <button
            onClick={nextQuestion}
            className="mx-auto px-8 py-4 rounded-xl bg-gradient-to-r from-purple-600 to-pink-600
              text-white text-lg font-bold hover:from-purple-500 hover:to-pink-500
              transition-all duration-200 hover:scale-[1.02]"
          >
            {questionNum >= questions.length ? '🏆 Finish Game' : '➡️ Next Question'}
          </button>
        )}

        {!isHost && revealed && (
          <p className="text-center text-gray-500 animate-pulse">
            Waiting for host...
          </p>
        )}
      </div>

      {/* Mini scoreboard */}
      <div className="mt-4 flex justify-center gap-4 flex-wrap">
        {players.slice(0, 5).map((p, i) => (
          <div key={p.id} className="flex items-center gap-2 bg-gray-800/50 rounded-full px-3 py-1 text-sm">
            <span>{i === 0 ? '👑' : ['🥈', '🥉', '4️⃣', '5️⃣'][i - 1]}</span>
            <span className={p.id === player.id ? 'text-yellow-400 font-bold' : 'text-gray-300'}>
              {p.name}
            </span>
            <span className="text-gray-500">{p.score}</span>
          </div>
        ))}
      </div>
    </div>
  )
}
