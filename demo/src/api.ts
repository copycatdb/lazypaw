import { createClient, type LazypawClient } from 'lazypaw-js'

// Auto-detect API URL: same host, port 3000
const API_URL = `http://${window.location.hostname}:3333`

export const lp: LazypawClient = createClient(API_URL)

// ─── Types ───────────────────────────────────────────────────────────

export interface Game {
  id: number
  code: string
  host_name: string
  status: 'waiting' | 'playing' | 'finished'
  current_question: number
  questions?: Question[]
  players?: Player[]
}

export interface Question {
  id: number
  game_id: number
  text: string
  option_a: string
  option_b: string
  option_c: string
  option_d: string
  correct: string
  order_num: number
}

export interface Player {
  id: number
  game_id: number
  name: string
  score: number
  joined_at: string
}

export interface Answer {
  id: number
  player_id: number
  question_id: number
  choice: string
  is_correct: boolean
  answered_at: string
}
