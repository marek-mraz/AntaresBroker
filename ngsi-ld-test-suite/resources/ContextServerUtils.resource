*** Settings ***
Documentation       Create a mock server to response to the Context Broker with the corresponding @context

Variables           ${EXECDIR}/resources/variables.py
Library             HttpCtrl.Server
Library             OperatingSystem


*** Variables ***
${filename}=    @context-minimal-valid.json


*** Keywords ***
Start @context Local Server
    [Arguments]    ${host}=${context_server_host}    ${port}=${context_server_port}

    # Initialize HTTP Server
    Start Server    ${host}    ${port}

    ${payload}=    Get File    ${EXECDIR}/data/jsonldContext/${filename}

    Set Stub Reply    GET    /api/v1/context.jsonld    200    ${payload}

Stop @context Local Server
    # Terminate HTTP Server
    Stop Server
