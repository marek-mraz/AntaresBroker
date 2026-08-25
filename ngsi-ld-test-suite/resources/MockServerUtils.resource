*** Settings ***
Documentation       Mock http server for subscriber

Library             HttpCtrl.Server
Variables           ${EXECDIR}/resources/variables.py


*** Keywords ***
Start Context Source Mock Server
    [Tags]    actor_remote-broker
    # Initialize HTTP Client And Server
    Start Server    ${context_source_host}    ${context_source_port}

Wait for redirected request
    [Tags]    actor_remote-broker
    [Arguments]    ${timeout}=${15}
    # HTTP server receives it and checks incoming request for correctness
    # .. "Wait For Request"
    # .... This call is blocked until HTTP request arrives or timeout.
    # .... Further detaills: https://annoviko.github.io/robotframework-httpctrl/server.html#Wait%20For%20Request
    Wait For Request    ${timeout}
    Reply By    200

Answer to Create Entity
    [Tags]    actor_remote-broker
    Set Stub Reply    POST    /ngsi-ld/v1/entities    201

Stop Context Source Mock Server
    [Tags]    actor_remote-broker
    # Terminate HTTP Server
    Stop Server
