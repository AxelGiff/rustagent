<script setup>
import { ref, reactive, computed } from 'vue'
import { questions, answers } from './data/cards.js'

// ===== ÉTAT DU JEU =====
const screen = ref('setup') // setup | game | end
const game = reactive({
  players: [],
  judgeIndex: 0,
  blackCard: null,
  currentRound: 0,
  maxRounds: 10,
  submitted: [],
  scores: {},
  roundPhase: 'answer' // answer | reveal | result
})

// Config setup
const playerNames = reactive([{ name: '' }, { name: '' }, { name: '' }])
const roundCount = ref(10)
let shuffledBlack = []
let shuffledWhite = []

// ===== FONCTIONS UTILITAIRES =====
function shuffle(arr) {
  const a = [...arr]
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[a[i], a[j]] = [a[j], a[i]]
  }
  return a
}

function startGame() {
  const players = playerNames
    .map(p => p.name.trim())
    .filter(n => n.length > 0 && n.length <= 20)
  if (players.length < 3) {
    alert('Il faut au moins 3 joueurs pour jouer à Blanc Manger Coco !')
    return
  }
  game.players = players
  game.maxRounds = roundCount.value
  game.scores = {}
  players.forEach(p => { game.scores[p] = 0 })
  shuffledBlack = shuffle(questions)
  shuffledWhite = shuffle(answers)
  game.currentRound = 0
  game.judgeIndex = 0
  screen.value = 'game'
  startRound()
}

function startRound() {
  if (game.currentRound >= game.maxRounds) {
    screen.value = 'end'
    return
  }
  game.currentRound++
  game.blackCard = shuffledBlack[(game.currentRound - 1) % shuffledBlack.length]
  game.submitted = []
  game.roundPhase = 'answer'
  game.judgeIndex = (game.currentRound - 1) % game.players.length
}

// Chaque joueur (non juge) choisit une réponse depuis sa main
const handSize = 7
function getHand(playerName) {
  const idx = game.players.indexOf(playerName)
  const start = (idx * handSize + game.currentRound * 3) % Math.max(shuffledWhite.length - handSize, 1)
  return shuffledWhite.slice(start, start + handSize)
}

function submitAnswer(playerName, card) {
  if (game.roundPhase !== 'answer') return
  if (playerName === game.players[game.judgeIndex]) return
  if (game.submitted.some(s => s.player === playerName)) return
  game.submitted.push({ player: playerName, card })
}

function judgePick(selected) {
  if (game.roundPhase !== 'reveal') return
  const winner = game.submitted[selected]
  if (!winner) return
  game.scores[winner.player]++
  game.roundPhase = 'result'
  game.lastWinner = winner.player
}

function nextRound() {
  startRound()
}

function resetGame() {
  screen.value = 'setup'
}

// ===== COMPUTED =====
const nonJudges = computed(() =>
  game.players.filter((_, i) => i !== game.judgeIndex)
)

const judgeName = computed(() => game.players[game.judgeIndex] || '')

const allSubmitted = computed(() =>
  game.submitted.length === nonJudges.value.length
)

const sortedScores = computed(() =>
  Object.entries(game.scores).sort((a, b) => b[1] - a[1])
)

function currentJudge() {
  return game.players[game.judgeIndex]
}
</script>

<template>
  <div id="bmc-app">
    <header class="app-header">
      <h1>🥥 Blanc Manger Coco</h1>
      <p class="tagline">Le jeu de cartes qui vous fera voir la vie en mauve</p>
    </header>

    <!-- ===== ÉCRAN DE CONFIGURATION ===== -->
    <main v-if="screen === 'setup'" class="setup-screen">
      <div class="setup-card">
        <h2>Configuration de la partie</h2>
        <div class="players-input">
          <h3>Joueurs (3 minimum)</h3>
          <div v-for="(p, i) in playerNames" :key="i" class="player-input-row">
            <span class="player-num">{{ i + 1 }}</span>
            <input
              v-model="p.name"
              :placeholder="'Nom du joueur ' + (i + 1)"
              maxlength="20"
            />
          </div>
        </div>
        <div class="settings-row">
          <label for="rounds">Nombre de manches :</label>
          <input
            id="rounds"
            type="number"
            v-model.number="roundCount"
            min="1"
            max="30"
          />
        </div>
        <button class="btn-primary" @click="startGame">🚀 Commencer la partie</button>
      </div>
    </main>

    <!-- ===== ÉCRAN DE JEU ===== -->
    <main v-else-if="screen === 'game'" class="game-screen">
      <div class="game-topbar">
        <div class="round-info">
          <span class="badge">Manche {{ game.currentRound }} / {{ game.maxRounds }}</span>
          <span class="judge-badge">👑 Juge : {{ judgeName }}</span>
        </div>
        <button class="btn-quit" @click="resetGame">✖ Quitter</button>
      </div>

      <div class="scores-bar">
        <div
          v-for="[name, score] in sortedScores"
          :key="name"
          class="score-chip"
          :class="{ judge: name === judgeName }"
        >
          <span class="score-name">{{ name }}</span>
          <span class="score-val">{{ score }} pts</span>
        </div>
      </div>

      <div class="black-card-zone" v-if="game.blackCard">
        <div class="black-card">
          <p>{{ game.blackCard.text }}</p>
          <span class="card-category">{{ game.blackCard.category || 'Question' }}</span>
          <span v-for="blank in game.blackCard.blanks || 1" :key="blank" class="blank-line"></span>
        </div>
      </div>

      <!-- Phase réponse : chaque joueur non juge choisit -->
      <div v-if="game.roundPhase === 'answer'" class="phase-answer">
        <h3>🔎 À vous de jouer ! Choisissez votre carte réponse</h3>
        <div v-for="name in nonJudges" :key="name" class="player-turn">
          <div class="turn-label">
            <strong>{{ name }}</strong>
            <span v-if="game.submitted.some(s => s.player === name)" class="ready-tag">✅ Prêt</span>
          </div>
          <div v-if="!game.submitted.some(s => s.player === name)" class="hand">
            <button
              v-for="card in getHand(name)"
              :key="card.id"
              class="white-card"
              @click="submitAnswer(name, card)"
            >
              {{ card.text }}
            </button>
          </div>
        </div>

        <div v-if="allSubmitted" class="continue-hint">
          <p>✅ Tous les joueurs ont répondu ! Passez l'écran au juge.</p>
          <button class="btn-primary" @click="game.roundPhase = 'reveal'">👑 Révéler au juge</button>
        </div>
      </div>

      <!-- Phase révélation : le juge choisit -->
      <div v-else-if="game.roundPhase === 'reveal'" class="phase-reveal">
        <h3>👑 {{ judgeName }}, choisissez la meilleure réponse !</h3>
        <div class="reveal-grid">
          <button
            v-for="(sub, i) in game.submitted"
            :key="i"
            class="reveal-card"
            @click="judgePick(i)"
          >
            <span class="reveal-anon">Auteur : {{ sub.player }}</span>
            <p>{{ sub.card.text }}</p>
          </button>
        </div>
      </div>

      <!-- Phase résultat -->
      <div v-else-if="game.roundPhase === 'result'" class="phase-result">
        <div class="result-banner">
          <span class="trophy">🏆</span>
          <h2>{{ game.lastWinner }} remporte la manche !</h2>
          <p>{{ judgeName }} a choisi sa réponse.</p>
        </div>
        <div class="result-actions">
          <button v-if="game.currentRound < game.maxRounds" class="btn-primary" @click="nextRound">
            ➡️ Manche suivante
          </button>
          <button v-else class="btn-primary" @click="screen = 'end'">
            🏁 Voir les résultats
          </button>
        </div>
      </div>
    </main>

    <!-- ===== ÉCRAN DE FIN ===== -->
    <main v-else-if="screen === 'end'" class="end-screen">
      <div class="end-card">
        <h2>🎉 Fin de la partie !</h2>
        <ol class="final-ranking">
          <li v-for="([name, score], i) in sortedScores" :key="name">
            <span class="rank">{{ i + 1 }}</span>
            <span class="rank-name">{{ name }}</span>
            <span class="rank-score">{{ score }} pts</span>
          </li>
        </ol>
        <button class="btn-primary" @click="resetGame">🔄 Rejouer</button>
      </div>
    </main>
  </div>
</template>
