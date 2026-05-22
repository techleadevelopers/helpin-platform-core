param(
  [string]$BaseUrl = "http://127.0.0.1:8080",
  [Parameter(Mandatory = $true)][string]$RoomId,
  [Parameter(Mandatory = $true)][string]$AccessToken,
  [int]$Vus = 100,
  [string]$Duration = "2m",
  [int]$SessionMs = 30000
)

$ErrorActionPreference = "Stop"
$env:BASE_URL = $BaseUrl
$env:ROOM_ID = $RoomId
$env:ACCESS_TOKEN = $AccessToken
$env:K6_WS_VUS = "$Vus"
$env:K6_DURATION = $Duration
$env:WS_SESSION_MS = "$SessionMs"

k6 run "$PSScriptRoot\..\k6\websocket-chat.js"
