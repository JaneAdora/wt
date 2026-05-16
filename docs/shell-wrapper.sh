# Source this from your shell rc to get the `wtcd` function.
wtcd() {
    local cmd
    cmd="$(wt)" || return 1
    if [ -z "$cmd" ]; then
        return 0
    fi
    eval "$cmd"
}
