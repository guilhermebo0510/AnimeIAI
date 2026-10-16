#!/bin/bash
# Reabre o app (com build), liga o vigia de mouse fisico e roda o roteiro de mouse real; para o vigia no fim.
cd /c/AnimeIAI
powershell -NoProfile -File tools/relanca.ps1 -Workspace animacao 2>&1 | tail -1
cd /c/AnimeIAI/sprints/registro/evidencias/S03/ui
rm -f watch.stop mouse-real-watch.json
(powershell -NoProfile -ExecutionPolicy Bypass -File /c/AnimeIAI/tools/harness/watch_input.ps1 -Out 'C:\AnimeIAI\sprints\registro\evidencias\S03\ui\mouse-real-watch.json' -StopFile 'C:\AnimeIAI\sprints\registro\evidencias\S03\ui\watch.stop' -MaxSeconds 900 > /dev/null 2>&1 &)
sleep 3
python mouse_real_s03.py 2>&1 | tail -${1:-60}
rc=${PIPESTATUS[0]}
touch watch.stop
sleep 3
rm -f watch.stop
cat mouse-real-watch.json 2>/dev/null | tr -d '\n' | head -c 400
echo
exit $rc
