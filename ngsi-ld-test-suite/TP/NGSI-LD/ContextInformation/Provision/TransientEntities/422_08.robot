*** Settings ***
Documentation       4.22/4.8 expiresAt write-site matrix: every location the spec
...                 lets a client write expiresAt — entity level, every attribute
...                 type (Property, Relationship, GeoProperty, LanguageProperty,
...                 VocabProperty, JsonProperty), sub-attributes, multi-instance
...                 datasetId — plus the non-entity resources that carry it
...                 (CSourceRegistration 5.2.9, Subscription 5.2.12) and the
...                 validation edges: expiresAt is a DateTime (4.6.3), so a
...                 malformed value is BadRequestData at every site; a past
...                 expiresAt on a registration or subscription is BadRequestData
...                 (5.9.2.4 / 5.8.1.4 by reference to it).
...
...                 Antares extension TP. Broker must run with a short sweep
...                 (ANTARES_SWEEP_SECS=2) for the lazy-expiry cases.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource

Suite Teardown      Purge Matrix State


*** Variables ***
${eid}=         urn:ngsi-ld:Vehicle:42208-types
${eid2}=        urn:ngsi-ld:Vehicle:42208-datasets
${eid3}=        urn:ngsi-ld:Vehicle:42208-past
${csr_id}=      urn:ngsi-ld:ContextSourceRegistration:42208
${sub_id}=      urn:ngsi-ld:Subscription:42208
${future}=      2100-01-01T00:00:00Z
${past}=        2020-01-01T00:00:00Z
${JSONH}=       ${{ {"Content-Type": "application/json"} }}


*** Test Cases ***
422_08_01 ExpiresAt Is Writable On Every Attribute Type
    [Documentation]    4.8: expiresAt is defined for "the Entity, Property,
    ...    Relationship" — every attribute type accepts it; with sysAttrs it is
    ...    served back on each; without sysAttrs none of them leak it (6.3.11).
    [Tags]    transient    4_22    since_v1.9.1
    ${payload}=    Evaluate    {"id": "${eid}", "type": "Vehicle", "expiresAt": "${future}", "prop": {"type": "Property", "value": 1, "expiresAt": "${future}", "sub": {"type": "Property", "value": 2, "expiresAt": "${future}"}}, "rel": {"type": "Relationship", "object": "urn:ngsi-ld:X:1", "expiresAt": "${future}"}, "geo": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}, "expiresAt": "${future}"}, "langp": {"type": "LanguageProperty", "languageMap": {"en": "hi"}, "expiresAt": "${future}"}, "vocab": {"type": "VocabProperty", "vocab": "stopped", "expiresAt": "${future}"}, "js": {"type": "JsonProperty", "json": {"a": 1}, "expiresAt": "${future}"}}
    ${response}=    POST    url=${url}/entities/    json=${payload}    headers=${JSONH}    expected_status=201
    ${response}=    GET    url=${url}/entities/${eid}    params=options=sysAttrs    expected_status=200
    ${body}=    Set Variable    ${response.json()}
    FOR    ${attr}    IN    prop    rel    geo    langp    vocab    js
        Should Be Equal    ${body}[${attr}][expiresAt]    ${future}
    END
    Should Be Equal    ${body}[prop][sub][expiresAt]    ${future}
    Should Be Equal    ${body}[expiresAt]    ${future}
    ${response}=    GET    url=${url}/entities/${eid}    expected_status=200
    Should Not Contain    ${response.text}    expiresAt

422_08_02 Dataset Instance Expires Independently Of The Default Instance
    [Documentation]    4.5.5: expiresAt is per attribute INSTANCE — a datasetId
    ...    instance with a short expiry vanishes while the default instance of
    ...    the same attribute survives.
    [Tags]    transient    4_22    since_v1.9.1
    ${soon}=    Evaluate    (__import__('datetime').datetime.now(__import__('datetime').timezone.utc) + __import__('datetime').timedelta(seconds=3)).strftime('%Y-%m-%dT%H:%M:%SZ')
    ${payload}=    Evaluate    {"id": "${eid2}", "type": "Vehicle", "speed": [{"type": "Property", "value": 10}, {"type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:d:1", "expiresAt": "${soon}"}]}
    ${response}=    POST    url=${url}/entities/    json=${payload}    headers=${JSONH}    expected_status=201
    ${response}=    GET    url=${url}/entities/${eid2}    expected_status=200
    Should Contain    ${response.text}    urn:ngsi-ld:d:1
    Sleep    4s
    ${response}=    GET    url=${url}/entities/${eid2}    expected_status=200
    Should Not Contain    ${response.text}    urn:ngsi-ld:d:1    msg=expired dataset instance must vanish
    Should Contain    ${response.text}    speed    msg=default instance must survive

422_08_03 Malformed ExpiresAt Is BadRequestData At Entity And Attribute Level
    [Documentation]    4.6.3: expiresAt is a DateTime — "not-a-date" and the
    ...    digit-shaped-but-impossible "2026-13-45T00:00:00Z" are both 400.
    [Tags]    transient    4_22    since_v1.9.1
    FOR    ${bad}    IN    not-a-date    2026-13-45T00:00:00Z
        ${payload}=    Evaluate    {"id": "${eid3}", "type": "Vehicle", "expiresAt": "${bad}"}
        ${response}=    POST    url=${url}/entities/    json=${payload}    headers=${JSONH}    expected_status=any
        Should Be Equal As Integers    ${response.status_code}    400
        Should Contain    ${response.text}    BadRequestData
        ${payload}=    Evaluate    {"id": "${eid3}", "type": "Vehicle", "speed": {"type": "Property", "value": 1, "expiresAt": "${bad}"}}
        ${response}=    POST    url=${url}/entities/    json=${payload}    headers=${JSONH}    expected_status=any
        Should Be Equal As Integers    ${response.status_code}    400
        Should Contain    ${response.text}    BadRequestData
    END

422_08_04 Past And Malformed ExpiresAt On A Registration Are BadRequestData
    [Documentation]    5.9.2.4: "If expiresAt is a date and time in the past, an
    ...    error of type BadRequestData shall be raised"; 5.2.9 types it DateTime.
    [Tags]    transient    4_22    5_9_2    since_v1.9.1
    FOR    ${bad}    IN    ${past}    2026-13-45T00:00:00Z
        ${payload}=    Evaluate    {"id": "${csr_id}", "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Vehicle"}]}], "endpoint": "http://csr.example.com", "expiresAt": "${bad}"}
        ${response}=    POST    url=${url}/csourceRegistrations/    json=${payload}    headers=${JSONH}    expected_status=any
        Should Be Equal As Integers    ${response.status_code}    400
        Should Contain    ${response.text}    BadRequestData
    END

422_08_05 A Registration Expires Lazily
    [Documentation]    5.9.2.4: "implementations shall delete the Registration
    ...    when this point in time is reached" — after expiry the registration
    ...    is gone from retrieval and discovery.
    [Tags]    transient    4_22    5_9_2    since_v1.9.1
    ${soon}=    Evaluate    (__import__('datetime').datetime.now(__import__('datetime').timezone.utc) + __import__('datetime').timedelta(seconds=3)).strftime('%Y-%m-%dT%H:%M:%SZ')
    ${payload}=    Evaluate    {"id": "${csr_id}", "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Vehicle"}]}], "endpoint": "http://csr.example.com", "expiresAt": "${soon}"}
    ${response}=    POST    url=${url}/csourceRegistrations/    json=${payload}    headers=${JSONH}    expected_status=201
    ${response}=    GET    url=${url}/csourceRegistrations/${csr_id}    expected_status=200
    Sleep    4s
    ${response}=    GET    url=${url}/csourceRegistrations/${csr_id}    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    404
    ${response}=    GET    url=${url}/csourceRegistrations/    params=type=Vehicle    expected_status=200
    Should Not Contain    ${response.text}    ${csr_id}

422_08_06 Past And Malformed ExpiresAt On A Subscription Are BadRequestData
    [Documentation]    5.8.1.4 (via 5.9.2.4's expiresAt rules) + 5.2.12 DateTime.
    [Tags]    transient    4_22    5_8_1    since_v1.9.1
    FOR    ${bad}    IN    ${past}    2026-13-45T00:00:00Z
        ${payload}=    Evaluate    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": {"endpoint": {"uri": "http://a.b.c/n"}}, "expiresAt": "${bad}"}
        ${response}=    POST    url=${url}/subscriptions/    json=${payload}    headers=${JSONH}    expected_status=any
        Should Be Equal As Integers    ${response.status_code}    400
        Should Contain    ${response.text}    BadRequestData
    END

422_08_07 A Subscription Expires Lazily
    [Documentation]    5.8.1.4/5.8.6: at expiresAt the subscription status
    ...    becomes "expired" and it stops notifying; it stays retrievable.
    [Tags]    transient    4_22    5_8_1    since_v1.9.1
    ${soon}=    Evaluate    (__import__('datetime').datetime.now(__import__('datetime').timezone.utc) + __import__('datetime').timedelta(seconds=3)).strftime('%Y-%m-%dT%H:%M:%SZ')
    ${payload}=    Evaluate    {"id": "${sub_id}", "type": "Subscription", "entities": [{"type": "Vehicle"}], "notification": {"endpoint": {"uri": "http://a.b.c/n"}}, "expiresAt": "${soon}"}
    ${response}=    POST    url=${url}/subscriptions/    json=${payload}    headers=${JSONH}    expected_status=201
    ${response}=    GET    url=${url}/subscriptions/${sub_id}    expected_status=200
    Should Contain    ${response.text}    ${sub_id}
    Sleep    4s
    # 5.8.6: unlike registrations, an expired subscription is NOT deleted —
    # its status becomes "expired" and notifications stop ("if and only if
    # the status ... is active, i.e. not paused nor expired").
    ${response}=    GET    url=${url}/subscriptions/${sub_id}    expected_status=200
    Should Be Equal    ${response.json()}[status]    expired
    Should Not Be Equal    ${response.json()}[status]    active

422_08_08 Entity Created Already Expired Is Never Served
    [Documentation]    4.22 has no past-value prohibition for entities — but
    ...    "reads never serve expired context": an entity created with a past
    ...    expiresAt must not be retrievable, whatever the create answered.
    [Tags]    transient    4_22    since_v1.9.1
    ${payload}=    Evaluate    {"id": "${eid3}", "type": "Vehicle", "expiresAt": "${past}", "speed": {"type": "Property", "value": 1}}
    ${response}=    POST    url=${url}/entities/    json=${payload}    headers=${JSONH}    expected_status=any
    Should Be True    ${response.status_code} in (201, 400)    create answers 201 (transient) or 400, never 5xx
    ${response}=    GET    url=${url}/entities/${eid3}    expected_status=any
    Should Be Equal As Integers    ${response.status_code}    404


*** Keywords ***
Purge Matrix State
    FOR    ${id}    IN    ${eid}    ${eid2}    ${eid3}
        ${response}=    DELETE    url=${url}/entities/${id}    expected_status=any
    END
    ${response}=    DELETE    url=${url}/csourceRegistrations/${csr_id}    expected_status=any
    ${response}=    DELETE    url=${url}/subscriptions/${sub_id}    expected_status=any
