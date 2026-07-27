#!/usr/bin/env bash
set -Eeuo pipefail
umask 077
readonly REPOSITORY="lexmarcos/sujiro-kimiskute"
readonly YT_DLP_REPOSITORY="yt-dlp/yt-dlp"
readonly USER_HOME="${HOME:-}"
readonly DEFAULT_INSTALL_DIR="${XDG_DATA_HOME:-${USER_HOME}/.local/share}/sujiro-kimiskute"
readonly DEFAULT_BIN_DIR="${USER_HOME}/.local/bin"
REQUESTED_VERSION="${SUJIRO_VERSION:-latest}"
INSTALL_DIR="${SUJIRO_INSTALL_DIR:-${DEFAULT_INSTALL_DIR}}"
BIN_DIR="${SUJIRO_BIN_DIR:-${DEFAULT_BIN_DIR}}"
TEMP_DIR=""
ROLLBACK_ACTIVE="false"
INSTALLATION_MODE="install"
PREVIOUS_VERSION=""
declare -a ENV_TEMPLATE_KEYS=()
declare -A ENV_TEMPLATE_LINES=()
print_step() {
    printf '\n\033[1;34m==>\033[0m %s\n' "$1"
}
print_success() {
    printf '\033[1;32m✓\033[0m %s\n' "$1"
}
print_warning() {
    printf '\033[1;33mWarning:\033[0m %s\n' "$1" >&2
}
fail_installation() {
    printf '\033[1;31mError:\033[0m %s\n' "$1" >&2
    exit 1
}
cleanup_temp_directory() {
    [[ -z "${TEMP_DIR}" || ! -d "${TEMP_DIR}" ]] || rm -rf -- "${TEMP_DIR}"
}

restore_destination() {
    local destination="$1" backup_name="$2"
    rm -f -- "${destination}"
    if [[ -e "${TEMP_DIR}/${backup_name}.previous" || -L "${TEMP_DIR}/${backup_name}.previous" ]]; then
        cp -a -- "${TEMP_DIR}/${backup_name}.previous" "${destination}"
    fi
}

finish_installation() {
    local exit_status=$? rollback_failed="false" staged_file
    trap - EXIT
    if ((exit_status != 0)) && [[ "${ROLLBACK_ACTIVE}" == "true" ]]; then
        restore_destination "${BOT_DESTINATION}" bot || rollback_failed="true"
        restore_destination "${ENV_DESTINATION}" env || rollback_failed="true"
        restore_destination "${LAUNCHER_DESTINATION}" launcher || rollback_failed="true"
        restore_destination "${VERSION_DESTINATION}" version || rollback_failed="true"
        [[ -z "${YT_DLP_CANDIDATE:-}" ]] \
            || restore_destination "${YT_DLP_PATH}" yt-dlp || rollback_failed="true"
        print_warning "The previous installation was restored after an error."
    fi
    for staged_file in \
        "${BOT_DESTINATION:-}" \
        "${ENV_DESTINATION:-}" \
        "${LAUNCHER_DESTINATION:-}" \
        "${VERSION_DESTINATION:-}"; do
        [[ -z "${staged_file}" ]] || rm -f -- "${staged_file}.new" || true
    done
    [[ -z "${YT_DLP_CANDIDATE:-}" || -z "${YT_DLP_PATH:-}" ]] \
        || rm -f -- "${YT_DLP_PATH}.new" || true
    [[ "${rollback_failed}" == "false" ]] || print_warning "Some rollback files could not be restored."
    cleanup_temp_directory
    exit "${exit_status}"
}
trap finish_installation EXIT
require_commands() {
    local command_name
    for command_name in curl tar sha256sum mktemp install uname sed awk grep head tail mv chmod mkdir dirname basename env timeout cp wc; do
        command -v "${command_name}" >/dev/null 2>&1 || {
            fail_installation "Required command not found: ${command_name}"
        }
    done
}
validate_installation_paths() {
    [[ -n "${USER_HOME}" ]] || fail_installation "HOME is not set."
    [[ -n "${INSTALL_DIR}" ]] || fail_installation "The installation directory cannot be empty."
    [[ -n "${BIN_DIR}" ]] || fail_installation "The launcher directory cannot be empty."
    [[ "${INSTALL_DIR}" == /* && "${BIN_DIR}" == /* ]] \
        || fail_installation "Installation paths must be absolute."
    [[ "${INSTALL_DIR}" != *$'\n'* && "${INSTALL_DIR}" != *$'\r'* ]] \
        || fail_installation "The installation directory contains an invalid line break."
    [[ "${BIN_DIR}" != *$'\n'* && "${BIN_DIR}" != *$'\r'* ]] \
        || fail_installation "The launcher directory contains an invalid line break."
}
detect_platform() {
    local operating_system architecture
    operating_system="$(uname -s)"
    architecture="$(uname -m)"
    [[ "${operating_system}" == "Linux" ]] || {
        fail_installation "Unsupported operating system: ${operating_system}. This installer supports Linux only."
    }
    case "${architecture}" in
        x86_64 | amd64)
            BOT_PLATFORM="linux-x86_64"
            YT_DLP_ASSET="yt-dlp_linux"
            ;;
        aarch64 | arm64)
            BOT_PLATFORM="linux-arm64"
            YT_DLP_ASSET="yt-dlp_linux_aarch64"
            ;;
        *)
            fail_installation "Unsupported architecture: ${architecture}. Supported: x86_64 and arm64."
            ;;
    esac
}

download_file() {
    local url="$1"
    local destination="$2"
    local partial_file="${destination}.partial"

    rm -f -- "${partial_file}"
    curl --fail --location --silent --show-error \
        --retry 3 --connect-timeout 15 --proto '=https' --tlsv1.2 \
        --output "${partial_file}" "${url}"
    mv -f -- "${partial_file}" "${destination}"
}

resolve_release_tag() {
    local metadata_file tag
    if [[ "${REQUESTED_VERSION}" != "latest" ]]; then
        tag="${REQUESTED_VERSION}"
    else
        metadata_file="${TEMP_DIR}/latest-release.json"
        download_file \
            "https://api.github.com/repos/${REPOSITORY}/releases/latest" \
            "${metadata_file}"
        tag="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
            "${metadata_file}" | head -n 1)"
    fi

    [[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$ ]] || {
        fail_installation "Invalid or unavailable release tag: ${tag:-empty}"
    }
    RELEASE_TAG="${tag}"
}

download_environment_template() {
    local template_size
    ENV_TEMPLATE="${TEMP_DIR}/.env.example"

    print_step "Downloading configuration template for ${RELEASE_TAG}"
    download_file \
        "https://raw.githubusercontent.com/${REPOSITORY}/${RELEASE_TAG}/.env.example" \
        "${ENV_TEMPLATE}"
    template_size="$(wc -c <"${ENV_TEMPLATE}")"
    ((template_size > 0 && template_size <= 65536)) || {
        fail_installation "The ${RELEASE_TAG} configuration template has an invalid size."
    }
    load_environment_template
    print_success "Configuration template for ${RELEASE_TAG} downloaded"
}

load_environment_template() {
    local line key
    while IFS= read -r line || [[ -n "${line}" ]]; do
        line="${line%$'\r'}"
        if [[ "${line}" =~ ^[[:space:]]*(export[[:space:]]+)?([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*= ]]; then
            key="${BASH_REMATCH[2]}"
            [[ -z "${ENV_TEMPLATE_LINES[${key}]+present}" ]] || {
                fail_installation "The ${RELEASE_TAG} configuration template repeats key ${key}."
            }
            ENV_TEMPLATE_KEYS+=("${key}")
            ENV_TEMPLATE_LINES["${key}"]="${line}"
            continue
        fi
        [[ "${line}" =~ ^[[:space:]]*$ || "${line}" =~ ^[[:space:]]*# ]] || {
            fail_installation "The ${RELEASE_TAG} configuration template contains an unsupported line."
        }
    done <"${ENV_TEMPLATE}"
    ((${#ENV_TEMPLATE_KEYS[@]} > 0)) || {
        fail_installation "The ${RELEASE_TAG} configuration template contains no settings."
    }
}

expected_checksum() {
    local checksum_file="$1"
    local asset_name="$2"
    awk -v expected_name="${asset_name}" '
        {
            file_name = $2
            sub(/^\*/, "", file_name)
            if (file_name == expected_name) {
                print $1
                matches++
            }
        }
        END { if (matches != 1) exit 1 }
    ' "${checksum_file}"
}

verify_download_checksum() {
    local asset_file="$1"
    local checksum_file="$2"
    local asset_name expected actual
    asset_name="$(basename -- "${asset_file}")"
    expected="$(expected_checksum "${checksum_file}" "${asset_name}")" || {
        fail_installation "Checksum file does not contain exactly one entry for ${asset_name}."
    }
    [[ "${expected}" =~ ^[A-Fa-f0-9]{64}$ ]] || {
        fail_installation "Invalid SHA-256 value for ${asset_name}."
    }
    actual="$(sha256sum -- "${asset_file}")"
    actual="${actual%% *}"
    [[ "${actual}" == "${expected}" ]] || {
        fail_installation "Checksum verification failed for ${asset_name}."
    }
}

download_bot_release() {
    local asset_name release_url archive_entries extraction_dir
    asset_name="sujiro-kimiskute-${RELEASE_TAG}-${BOT_PLATFORM}.tar.gz"
    release_url="https://github.com/${REPOSITORY}/releases/download/${RELEASE_TAG}"
    BOT_ARCHIVE="${TEMP_DIR}/${asset_name}"
    BOT_CHECKSUM="${BOT_ARCHIVE}.sha256"

    print_step "Downloading Sujiro Kimiskute ${RELEASE_TAG} (${BOT_PLATFORM})"
    download_file "${release_url}/${asset_name}" "${BOT_ARCHIVE}"
    download_file "${release_url}/${asset_name}.sha256" "${BOT_CHECKSUM}"
    verify_download_checksum "${BOT_ARCHIVE}" "${BOT_CHECKSUM}"

    archive_entries="$(tar -tzf "${BOT_ARCHIVE}")"
    [[ "${archive_entries}" == "sujiro-kimiskute" ]] || {
        fail_installation "The bot archive contains unexpected files."
    }

    extraction_dir="${TEMP_DIR}/bot"
    mkdir -p -- "${extraction_dir}"
    tar --extract --gzip --file "${BOT_ARCHIVE}" \
        --directory "${extraction_dir}" --no-same-owner --no-same-permissions
    BOT_CANDIDATE="${extraction_dir}/sujiro-kimiskute"
    [[ -f "${BOT_CANDIDATE}" && ! -L "${BOT_CANDIDATE}" ]] || {
        fail_installation "The bot archive did not contain a regular executable."
    }
    chmod 0755 "${BOT_CANDIDATE}"
    validate_bot_runtime
    print_success "Bot archive and checksum verified"
}

validate_bot_runtime() {
    local output exit_status
    set +e
    output="$(
        cd "${TEMP_DIR}/bot"
        timeout 5 env -i HOME="${USER_HOME}" PATH="/usr/bin:/bin" "${BOT_CANDIDATE}" 2>&1
    )"
    exit_status=$?
    set -e
    if ((exit_status != 1)) \
        || ! grep -Fq 'required environment variable DISCORD_TOKEN is missing or empty' <<<"${output}"; then
        fail_installation "This ${BOT_PLATFORM} build failed its runtime compatibility check."
    fi
}

absolute_command_path() {
    local command_path="$1"
    local command_directory
    command_directory="$(cd -P -- "$(dirname -- "${command_path}")" && pwd)"
    printf '%s/%s' "${command_directory}" "$(basename -- "${command_path}")"
}

find_or_download_yt_dlp() {
    local existing_path
    existing_path="$(type -P yt-dlp || true)"
    if [[ -n "${existing_path}" ]] && "${existing_path}" --version >/dev/null 2>&1; then
        YT_DLP_PATH="$(absolute_command_path "${existing_path}")"
        print_success "Using yt-dlp at ${YT_DLP_PATH}"
        return
    fi

    download_yt_dlp
}

download_yt_dlp() {
    local release_url checksum_list expected actual
    release_url="https://github.com/${YT_DLP_REPOSITORY}/releases/latest/download"
    YT_DLP_CANDIDATE="${TEMP_DIR}/${YT_DLP_ASSET}"
    checksum_list="${TEMP_DIR}/SHA2-256SUMS"

    print_step "yt-dlp was not found; downloading the official ${YT_DLP_ASSET} build"
    download_file "${release_url}/${YT_DLP_ASSET}" "${YT_DLP_CANDIDATE}"
    download_file "${release_url}/SHA2-256SUMS" "${checksum_list}"
    expected="$(expected_checksum "${checksum_list}" "${YT_DLP_ASSET}")" || {
        fail_installation "The official yt-dlp checksum list is missing ${YT_DLP_ASSET}."
    }
    [[ "${expected}" =~ ^[A-Fa-f0-9]{64}$ ]] || {
        fail_installation "The official yt-dlp checksum is invalid."
    }
    actual="$(sha256sum -- "${YT_DLP_CANDIDATE}")"
    actual="${actual%% *}"
    [[ "${actual}" == "${expected}" ]] || fail_installation "yt-dlp checksum verification failed."

    chmod 0755 "${YT_DLP_CANDIDATE}"
    "${YT_DLP_CANDIDATE}" --version >/dev/null 2>&1 || {
        fail_installation "The downloaded yt-dlp executable could not run on this system."
    }
    YT_DLP_PATH="${INSTALL_DIR}/bin/yt-dlp"
    print_success "Official yt-dlp download verified"
}

prompt_text() {
    local label="$1" default_value="$2" response
    if [[ -n "${default_value}" ]]; then
        printf '%s [%s]: ' "${label}" "${default_value}" >/dev/tty
    else
        printf '%s: ' "${label}" >/dev/tty
    fi
    IFS= read -r response </dev/tty
    printf '%s' "${response:-${default_value}}"
}

prompt_secret() {
    local label="$1" response
    printf '%s: ' "${label}" >/dev/tty
    IFS= read -r -s response </dev/tty
    printf '\n' >/dev/tty
    printf '%s' "${response}"
}

prompt_required_secret() {
    local label="$1" response
    while true; do
        response="$(prompt_secret "${label}")"
        if [[ -n "${response//[[:space:]]/}" ]]; then
            printf '%s' "${response}"
            return
        fi
        print_warning "This value is required."
    done
}

prompt_positive_integer() {
    local label="$1" default_value="$2" response
    while true; do
        response="$(prompt_text "${label}" "${default_value}")"
        if [[ "${response}" =~ ^[1-9][0-9]*$ ]] && ((${#response} <= 19)); then
            printf '%s' "${response}"
            return
        fi
        print_warning "Enter a positive integer."
    done
}

prompt_choice() {
    local label="$1" default_value="$2" allowed_values="$3" response allowed
    while true; do
        response="$(prompt_text "${label}" "${default_value}")"
        for allowed in ${allowed_values}; do
            if [[ "${response}" == "${allowed}" ]]; then
                printf '%s' "${response}"
                return
            fi
        done
        print_warning "Choose one of: ${allowed_values}"
    done
}

prompt_yes_no() {
    local label="$1" response
    while true; do
        printf '%s [y/N]: ' "${label}" >/dev/tty
        IFS= read -r response </dev/tty
        case "${response}" in
            y | Y | yes | YES) return 0 ;;
            "" | n | N | no | NO) return 1 ;;
            *) print_warning "Answer yes or no." ;;
        esac
    done
}

require_interactive_terminal() {
    (: <>/dev/tty) 2>/dev/null || {
        fail_installation "An interactive terminal is required to configure the bot securely."
    }
}

collect_configuration() {
    require_interactive_terminal
    print_step "Discord configuration"
    printf 'Create the bot at https://discord.com/developers/applications\n'
    DISCORD_TOKEN="$(prompt_required_secret "Discord bot token")"
    DISCORD_APPLICATION_ID="$(prompt_positive_integer "Discord Application ID (Client ID)" "")"
    BOT_LANGUAGE="$(prompt_choice "Bot language (pt-BR/en-US)" "pt-BR" "pt-BR en-US")"
    BOT_ACTIVITY_TYPE="$(prompt_choice "Discord activity type" "listening" \
        "listening playing watching competing")"
    BOT_ACTIVITY_MESSAGE="$(prompt_text "Discord activity message" "música")"
    [[ -n "${BOT_ACTIVITY_MESSAGE//[[:space:]]/}" ]] || {
        fail_installation "The Discord activity message cannot be empty."
    }

    YT_DLP_EXTRA_ARGS=""
    YT_DLP_TIMEOUT_SECONDS="20"
    AUTO_LEAVE_SECONDS="120"
    MAX_QUEUE_SIZE="50"
    MAX_CONCURRENT_RESOLUTIONS="4"
    RUST_LOG="info"

    if prompt_yes_no "Configure advanced settings?"; then
        YT_DLP_EXTRA_ARGS="$(prompt_secret "Extra yt-dlp arguments (optional)")"
        YT_DLP_TIMEOUT_SECONDS="$(prompt_positive_integer \
            "yt-dlp timeout in seconds" "${YT_DLP_TIMEOUT_SECONDS}")"
        AUTO_LEAVE_SECONDS="$(prompt_positive_integer \
            "Leave an empty voice channel after N seconds" "${AUTO_LEAVE_SECONDS}")"
        MAX_QUEUE_SIZE="$(prompt_positive_integer \
            "Maximum queue size" "${MAX_QUEUE_SIZE}")"
        MAX_CONCURRENT_RESOLUTIONS="$(prompt_positive_integer \
            "Maximum concurrent yt-dlp resolutions" "${MAX_CONCURRENT_RESOLUTIONS}")"
        RUST_LOG="$(prompt_text "RUST_LOG filter" "${RUST_LOG}")"
        [[ "${RUST_LOG}" =~ ^[A-Za-z0-9_:-]+(=(off|error|warn|info|debug|trace))?(,[A-Za-z0-9_:-]+(=(off|error|warn|info|debug|trace))?)*$ ]] \
            || fail_installation "RUST_LOG must contain valid target=level directives."
    fi

    write_configuration_file
}

escape_env_value() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    value="${value//\$/\\\$}"
    printf '%s' "${value}"
}

write_env_entry() {
    local key="$1" value="$2"
    [[ "${value}" != *$'\n'* && "${value}" != *$'\r'* ]] || {
        fail_installation "${key} contains an invalid line break."
    }
    printf '%s="%s"\n' "${key}" "$(escape_env_value "${value}")" >>"${CONFIG_CANDIDATE}"
}

write_configuration_file() {
    CONFIG_CANDIDATE="${TEMP_DIR}/sujiro.env"
    : >"${CONFIG_CANDIDATE}"
    chmod 0600 "${CONFIG_CANDIDATE}"
    write_env_entry DISCORD_TOKEN "${DISCORD_TOKEN}"
    write_env_entry DISCORD_APPLICATION_ID "${DISCORD_APPLICATION_ID}"
    write_env_entry BOT_LANGUAGE "${BOT_LANGUAGE}"
    write_env_entry BOT_ACTIVITY_TYPE "${BOT_ACTIVITY_TYPE}"
    write_env_entry BOT_ACTIVITY_MESSAGE "${BOT_ACTIVITY_MESSAGE}"
    write_env_entry BOT_ACTIVITY_CURRENT_TRACK "false"
    write_env_entry YT_DLP_PATH "${YT_DLP_PATH}"
    write_env_entry YT_DLP_EXTRA_ARGS "${YT_DLP_EXTRA_ARGS}"
    write_env_entry YT_DLP_TIMEOUT_SECONDS "${YT_DLP_TIMEOUT_SECONDS}"
    write_env_entry AUTO_LEAVE_SECONDS "${AUTO_LEAVE_SECONDS}"
    write_env_entry IDLE_LEAVE_SECONDS "300"
    write_env_entry PLAYER_PANEL_UPDATE_SECONDS "5"
    write_env_entry MAX_QUEUE_SIZE "${MAX_QUEUE_SIZE}"
    write_env_entry MAX_CONCURRENT_RESOLUTIONS "${MAX_CONCURRENT_RESOLUTIONS}"
    write_env_entry RUST_LOG "${RUST_LOG}"
}

prepare_installation_destinations() {
    [[ ! -L "${INSTALL_DIR}" ]] || fail_installation "The installation directory cannot be a symlink."
    if [[ ! -d "${INSTALL_DIR}" ]]; then
        mkdir -p -- "${INSTALL_DIR}"
        chmod 0700 "${INSTALL_DIR}"
    fi
    mkdir -p -- "${INSTALL_DIR}/bin" "${BIN_DIR}"
    [[ -w "${INSTALL_DIR}/bin" && -w "${BIN_DIR}" ]] || {
        fail_installation "The installation directories are not writable."
    }
    BOT_DESTINATION="${INSTALL_DIR}/bin/sujiro-kimiskute"
    ENV_DESTINATION="${INSTALL_DIR}/.env"
    LAUNCHER_DESTINATION="${BIN_DIR}/sujiro-kimiskute"
    VERSION_DESTINATION="${INSTALL_DIR}/.install-version"
    [[ "${BOT_DESTINATION}" != "${LAUNCHER_DESTINATION}" ]] \
        || fail_installation "The launcher and application binary cannot use the same path."
}

detect_installation_state() {
    local destination
    for destination in \
        "${BOT_DESTINATION}" \
        "${ENV_DESTINATION}" \
        "${LAUNCHER_DESTINATION}" \
        "${VERSION_DESTINATION}"; do
        if [[ -e "${destination}" || -L "${destination}" ]]; then
            INSTALLATION_MODE="upgrade"
            break
        fi
    done
    [[ "${INSTALLATION_MODE}" == "upgrade" ]] || return 0

    if [[ ! -e "${VERSION_DESTINATION}" && ! -L "${VERSION_DESTINATION}" ]]; then
        print_warning "Existing installation has no version metadata; the previous version is unknown."
        return
    fi
    [[ -f "${VERSION_DESTINATION}" && ! -L "${VERSION_DESTINATION}" ]] || {
        print_warning "Existing version metadata is not a regular file; the previous version is unknown."
        return
    }
    PREVIOUS_VERSION="$(<"${VERSION_DESTINATION}")"
    if [[ ! "${PREVIOUS_VERSION}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$ ]]; then
        print_warning "Existing version metadata is invalid; the previous version is unknown."
        PREVIOUS_VERSION=""
    fi
}

collect_present_environment_keys() {
    local environment_file="$1" line key
    declare -gA PRESENT_ENV_KEYS=()
    while IFS= read -r line || [[ -n "${line}" ]]; do
        line="${line%$'\r'}"
        if [[ "${line}" =~ ^[[:space:]]*(export[[:space:]]+)?([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*= ]]; then
            key="${BASH_REMATCH[2]}"
            PRESENT_ENV_KEYS["${key}"]="present"
        fi
    done <"${environment_file}"
}

prompt_missing_environment_value() {
    local key="$1" template_line="$2" response
    case "${key}" in
        DISCORD_TOKEN)
            response="$(prompt_required_secret "Discord bot token (${key})")"
            write_env_entry "${key}" "${response}"
            ;;
        DISCORD_APPLICATION_ID)
            response="$(prompt_positive_integer "Discord Application ID (${key})" "")"
            write_env_entry "${key}" "${response}"
            ;;
        YT_DLP_PATH)
            response="$(prompt_text "${key}" "${YT_DLP_PATH}")"
            write_env_entry "${key}" "${response}"
            ;;
        *TOKEN* | *SECRET* | *PASSWORD* | *PRIVATE_KEY* | *COOKIE*)
            response="$(prompt_secret "${key} (Enter uses the release default)")"
            if [[ -n "${response}" ]]; then
                write_env_entry "${key}" "${response}"
            else
                printf '%s\n' "${template_line}" >>"${CONFIG_CANDIDATE}"
            fi
            ;;
        *)
            response="$(prompt_text "${key} (Enter uses the release default)" "")"
            if [[ -n "${response}" ]]; then
                write_env_entry "${key}" "${response}"
            else
                printf '%s\n' "${template_line}" >>"${CONFIG_CANDIDATE}"
            fi
            ;;
    esac
}

append_missing_environment_keys() {
    local key last_byte missing_count=0
    collect_present_environment_keys "${CONFIG_CANDIDATE}"
    for key in "${ENV_TEMPLATE_KEYS[@]}"; do
        [[ -n "${PRESENT_ENV_KEYS[${key}]+present}" ]] || ((missing_count += 1))
    done
    if ((missing_count == 0)); then
        print_success "Existing configuration already contains every ${RELEASE_TAG} setting"
        return
    fi

    require_interactive_terminal
    print_step "Configuring ${missing_count} new setting(s) from ${RELEASE_TAG}"
    if [[ -s "${CONFIG_CANDIDATE}" ]]; then
        last_byte="$(tail -c 1 -- "${CONFIG_CANDIDATE}")"
        [[ -z "${last_byte}" ]] || printf '\n' >>"${CONFIG_CANDIDATE}"
    fi
    printf '\n# Added while upgrading to %s\n' "${RELEASE_TAG}" >>"${CONFIG_CANDIDATE}"
    for key in "${ENV_TEMPLATE_KEYS[@]}"; do
        [[ -z "${PRESENT_ENV_KEYS[${key}]+present}" ]] || continue
        prompt_missing_environment_value "${key}" "${ENV_TEMPLATE_LINES[${key}]}"
    done
}

prepare_configuration() {
    if [[ "${INSTALLATION_MODE}" == "install" ]]; then
        collect_configuration
        append_missing_environment_keys
        return
    fi

    CONFIG_CANDIDATE="${TEMP_DIR}/sujiro.env"
    if [[ -e "${ENV_DESTINATION}" || -L "${ENV_DESTINATION}" ]]; then
        [[ -f "${ENV_DESTINATION}" && ! -L "${ENV_DESTINATION}" ]] || {
            fail_installation "Refusing to read non-regular configuration: ${ENV_DESTINATION}"
        }
        cp -a -- "${ENV_DESTINATION}" "${CONFIG_CANDIDATE}"
        chmod 0600 "${CONFIG_CANDIDATE}"
    else
        : >"${CONFIG_CANDIDATE}"
        chmod 0600 "${CONFIG_CANDIDATE}"
        print_warning "Existing installation has no .env; all release settings must be configured."
    fi
    append_missing_environment_keys
}

backup_destination() {
    local destination="$1" backup_name="$2"
    if [[ -e "${destination}" || -L "${destination}" ]]; then
        [[ -f "${destination}" || -L "${destination}" ]] || {
            fail_installation "Refusing to replace non-file destination: ${destination}"
        }
        cp -a -- "${destination}" "${TEMP_DIR}/${backup_name}.previous"
    fi
}

install_verified_files() {
    local launcher_candidate="${TEMP_DIR}/sujiro-launcher"
    local version_candidate="${TEMP_DIR}/install-version"
    print_step "Installing verified files"
    rm -f -- \
        "${BOT_DESTINATION}.new" \
        "${ENV_DESTINATION}.new" \
        "${LAUNCHER_DESTINATION}.new" \
        "${VERSION_DESTINATION}.new"
    [[ -z "${YT_DLP_CANDIDATE:-}" ]] || rm -f -- "${YT_DLP_PATH}.new"
    install -m 0755 "${BOT_CANDIDATE}" "${BOT_DESTINATION}.new"
    if [[ -n "${YT_DLP_CANDIDATE:-}" ]]; then
        install -m 0755 "${YT_DLP_CANDIDATE}" "${YT_DLP_PATH}.new"
    fi
    install -m 0600 "${CONFIG_CANDIDATE}" "${ENV_DESTINATION}.new"
    printf '#!/usr/bin/env bash\nset -Eeuo pipefail\ncd -- %q\nexec %q "$@"\n' \
        "${INSTALL_DIR}" "${BOT_DESTINATION}" >"${launcher_candidate}"
    install -m 0755 "${launcher_candidate}" "${LAUNCHER_DESTINATION}.new"
    printf '%s\n' "${RELEASE_TAG}" >"${version_candidate}"
    install -m 0600 "${version_candidate}" "${VERSION_DESTINATION}.new"

    backup_destination "${BOT_DESTINATION}" bot
    backup_destination "${ENV_DESTINATION}" env
    backup_destination "${LAUNCHER_DESTINATION}" launcher
    backup_destination "${VERSION_DESTINATION}" version
    [[ -z "${YT_DLP_CANDIDATE:-}" ]] || backup_destination "${YT_DLP_PATH}" yt-dlp
    ROLLBACK_ACTIVE="true"
    mv -f -- "${BOT_DESTINATION}.new" "${BOT_DESTINATION}"
    [[ -z "${YT_DLP_CANDIDATE:-}" ]] || mv -f -- "${YT_DLP_PATH}.new" "${YT_DLP_PATH}"
    mv -f -- "${ENV_DESTINATION}.new" "${ENV_DESTINATION}"
    mv -f -- "${LAUNCHER_DESTINATION}.new" "${LAUNCHER_DESTINATION}"
    mv -f -- "${VERSION_DESTINATION}.new" "${VERSION_DESTINATION}"
    ROLLBACK_ACTIVE="false"
    LAUNCHER_PATH="${LAUNCHER_DESTINATION}"
    print_success "Installed Sujiro Kimiskute ${RELEASE_TAG}"
}

print_completion() {
    local invite_url
    if [[ "${INSTALLATION_MODE}" == "upgrade" ]]; then
        printf '\nUpgrade complete.\n'
        printf '  Version:       %s -> %s\n' "${PREVIOUS_VERSION:-unknown (legacy installation)}" "${RELEASE_TAG}"
    else
        invite_url="https://discord.com/oauth2/authorize?client_id=${DISCORD_APPLICATION_ID}&permissions=3148800&scope=bot%20applications.commands"
        printf '\nInstallation complete.\n'
        printf '  Version:       %s\n' "${RELEASE_TAG}"
    fi
    printf '  Platform:      %s\n' "${BOT_PLATFORM}"
    printf '  Installation:  %s\n' "${INSTALL_DIR}"
    printf '  Configuration: %s/.env\n' "${INSTALL_DIR}"
    printf '  yt-dlp:        %s\n' "${YT_DLP_PATH}"
    [[ "${INSTALLATION_MODE}" == "upgrade" ]] || printf '  Invite bot:    %s\n' "${invite_url}"
    printf '\nStart the bot with:\n  %s\n' "${LAUNCHER_PATH}"

    case ":${PATH}:" in
        *":${BIN_DIR}:"*) ;;
        *) print_warning "${BIN_DIR} is not in PATH. Use the full command above or add it to PATH." ;;
    esac
}

main() {
    if (($# > 0)); then
        printf 'Usage: SUJIRO_VERSION=v0.1.1 SUJIRO_INSTALL_DIR=/path SUJIRO_BIN_DIR=/path ./install.sh\n'
        exit 0
    fi
    validate_installation_paths
    require_commands
    TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/sujiro-install.XXXXXXXX")"
    detect_platform
    resolve_release_tag
    download_bot_release
    download_environment_template
    find_or_download_yt_dlp
    prepare_installation_destinations
    detect_installation_state
    prepare_configuration
    install_verified_files
    print_completion
}

main "$@"
