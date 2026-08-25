*** Settings ***
Documentation       6.3.11 Table 6.3.11-1: "When its value includes the keyword
...                 sysAttrs ... the system generated temporal attributes createdAt,
...                 modifiedAt and the system temporal attribute expiresAt are
...                 included in the response payload body where known." — so
...                 WITHOUT sysAttrs, expiresAt must NOT appear: not at entity
...                 level, not on attributes, not on temporal instances.
...
...                 Antares extension TP — the official sysAttrs TPs (018_11,
...                 018_12) cover createdAt/modifiedAt only.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource

Suite Setup         Create Transient Entity
Suite Teardown      Delete Transient Entity


*** Variables ***
${entity_id}=       urn:ngsi-ld:Vehicle:6311-transient


*** Test Cases ***
6311_01_01 ExpiresAt Is Absent Without SysAttrs
    [Documentation]    6.3.11: expiresAt is a system temporal attribute — a plain
    ...    retrieve must not include it at entity or attribute level.
    [Tags]    e-retrieve    6_3_11    since_v1.9.1
    ${response}=    GET    url=${url}/entities/${entity_id}    expected_status=200
    Should Not Contain    ${response.text}    expiresAt
    Should Contain    ${response.text}    speed

6311_01_02 ExpiresAt Is Included With SysAttrs
    [Documentation]    6.3.11: with options=sysAttrs the stored expiresAt is
    ...    included where known — entity level and attribute level.
    [Tags]    e-retrieve    6_3_11    since_v1.9.1
    ${response}=    GET    url=${url}/entities/${entity_id}
    ...    params=options=sysAttrs    expected_status=200
    ${body}=    Set Variable    ${response.json()}
    Should Be Equal    ${body}[expiresAt]    2100-01-01T00:00:00Z
    Should Be Equal    ${body}[speed][expiresAt]    2100-01-01T00:00:00Z
    Should Contain    ${response.text}    createdAt

6311_01_03 Temporal Instances Follow The Same Gate
    [Documentation]    6.3.11 covers /temporal/entities and sub-resources: an
    ...    instance's expiresAt appears only with options=sysAttrs.
    [Tags]    e-retrieve    6_3_11    since_v1.9.1
    ${response}=    GET    url=${url}/temporal/entities/${entity_id}
    ...    params=timerel=after&timeAt=2020-01-01T00:00:00Z    expected_status=any
    Should Be True    ${response.status_code} in (200, 206)
    Should Not Contain    ${response.text}    expiresAt
    ${response}=    GET    url=${url}/temporal/entities/${entity_id}
    ...    params=timerel=after&timeAt=2020-01-01T00:00:00Z&options=sysAttrs
    ...    expected_status=any
    Should Be True    ${response.status_code} in (200, 206)
    Should Contain    ${response.text}    expiresAt


*** Keywords ***
Create Transient Entity
    ${payload}=    Evaluate
    ...    {"id": "${entity_id}", "type": "Vehicle", "expiresAt": "2100-01-01T00:00:00Z", "speed": {"type": "Property", "value": 7, "observedAt": "2026-08-13T00:00:00Z", "expiresAt": "2100-01-01T00:00:00Z"}}
    ${response}=    POST    url=${url}/entities/    json=${payload}
    ...    headers=${{ {"Content-Type": "application/json"} }}    expected_status=201

Delete Transient Entity
    ${response}=    DELETE    url=${url}/entities/${entity_id}    expected_status=any
