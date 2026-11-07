#!/usr/bin/env zsh

# function definition
some_function() {
  echo "plain function"
  echo "more body"
}

# nested function definitions
outer() {
  inner() {
    echo "nested"
    echo "nested again"
  }
  inner
}

# if / elif / else
if [[ -n $1 ]]; then
  echo "branch one"
elif [[ -n $2 ]]; then
  echo "branch two"
else
  echo "fallback"
fi

# brace group
{
  echo "grouped one"
  echo "grouped two"
}

# brace group with always
{
  echo "try"
} always {
  echo "cleanup"
  echo "cleanup again"
}

# for: standard
for item in alpha beta gamma; do
  echo "$item"
done

# for: terse
for item (alpha beta gamma) {
  echo "$item"
  echo "again"
}

# for: c-style
for ((i = 0; i < 3; i++)); do
  echo "$i"
done

# while
while true; do
  echo "loop"
  break
done

# until
until false; do
  echo "loop"
  break
done

# repeat
repeat 3 {
  echo "again"
  echo "again again"
}

# select
select choice in yes no; do
  echo "$choice"
  break
done

# case
case $1 in
  start)
    echo "starting"
    echo "started"
    ;;
  stop)
    echo "stopping"
    ;;
  *)
    echo "unknown"
    echo "really"
    ;;
esac
