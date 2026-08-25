*** Settings ***
Documentation       Verify 5.16 Snapshots (optional API group; resources
...                 6.36 /snapshots, 6.37 /snapshots/{id},
...                 6.38 /snapshots/{id}/clone; 6.3.22 NGSILD-Snapshot
...                 scoping). Create executes the snapshotQueries into an
...                 isolated frozen copy; status/update/clone/delete/purge
...                 follow 5.16.2-5.16.7. Antares extension TP — the
...                 official suite has no Snapshot coverage.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Fixture Entities
Test Teardown       Clean Up


*** Variables ***
${fast}=    urn:ngsi-ld:Vehicle:snapfast
${slow}=    urn:ngsi-ld:Vehicle:snapslow


*** Test Cases ***
5161_01_01 Create Snapshot Freezes The Query Results
    [Documentation]    5.16.1.4: 201 + Location; status preparing→success
    ...    with per-query details; 6.3.22: NGSILD-Snapshot scopes queries to
    ...    the frozen copy (header echoed), and later changes to the live
    ...    entity must NOT appear in the snapshot.
    [Tags]    snapshots    5_16_1    6_3_22    since_v1.9.1

    ${loc}=    Create Snapshot    {"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>50"}]}
    ${snapshot}=    Wait Until Ready    ${loc}
    Should Be Equal    ${snapshot}[snapshotStatus]    success
    Should Be Equal As Integers    ${snapshot}[snapshotPriority]    5
    Should Be Equal    ${snapshot}[snapshotQueriesDetails][0][resultStatus]    success
    ${sid}=    Set Variable    ${snapshot}[id]

    &{sheaders}=    Create Dictionary    NGSILD-Snapshot=${sid}
    ${response}=    GET    url=${url}/entities    params=type=Vehicle    headers=${sheaders}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${echo}=    Get From Dictionary    ${response.headers}    NGSILD-Snapshot
    Should Be Equal    ${echo}    ${sid}
    ${ids}=    Evaluate    sorted(e["id"] for e in $response.json())
    Should Be Equal As Strings    ${ids}    ['${fast}']
    Should Not Contain    ${ids}    ${slow}

    # freeze proof: mutate the live entity, the snapshot copy stays put
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH    url=${url}/entities/${fast}/attrs
    ...    data={"speed": {"type": "Property", "value": 10}}    headers=${headers}    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    GET    url=${url}/entities/${fast}    headers=${sheaders}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()}[speed][value]    80

    Delete Snapshot    ${loc}

5161_01_02 Snapshot Validation And Unknown References
    [Documentation]    5.2.41: no queries member → 400; temporalQ inside
    ...    snapshotQueries → 400; 6.3.22: an unknown NGSILD-Snapshot → 404.
    [Tags]    snapshots    5_16_1    6_3_22    since_v1.9.1

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${url}/snapshots    data={"type": "Snapshot"}    headers=${headers}    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    ${response}=    POST    url=${url}/snapshots
    ...    data={"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": "V"}], "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}]}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    400    ${response.status_code}

    &{sheaders}=    Create Dictionary    NGSILD-Snapshot=urn:ngsi-ld:snapshot:absent
    ${response}=    GET    url=${url}/entities    params=type=Vehicle    headers=${sheaders}    expected_status=any
    Check Response Status Code    404    ${response.status_code}

5161_01_03 Update Clone Delete And Purge
    [Documentation]    5.16.4: PATCH updates snapshotPriority, read-only
    ...    members are 400; 5.16.2: the clone survives the original's
    ...    delete; 5.16.7: purge by q removes only matching snapshots.
    [Tags]    snapshots    5_16_2    5_16_4    5_16_5    5_16_7    since_v1.9.1

    ${loc}=    Create Snapshot    {"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]}
    ${snapshot}=    Wait Until Ready    ${loc}

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH    url=${url}${loc}    data={"snapshotPriority": 2}    headers=${headers}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Integers    ${response.json()}[snapshotPriority]    2
    ${response}=    PATCH    url=${url}${loc}    data={"snapshotQueries": []}    headers=${headers}    expected_status=any
    Check Response Status Code    400    ${response.status_code}

    ${response}=    POST    url=${url}${loc}/clone    data={"type": "Snapshot", "snapshotPriority": 9}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${cloc}=    Get From Dictionary    ${response.headers}    Location
    ${cloc}=    Evaluate    "${cloc}".replace("/ngsi-ld/v1", "")
    ${clone}=    Wait Until Ready    ${cloc}
    Should Be Equal    ${clone}[snapshotStatus]    success

    # 5.16.7: purge priority<5 removes the original, keeps the clone
    ${response}=    DELETE    url=${url}/snapshots    params=q=snapshotPriority<5    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    GET    url=${url}${loc}    expected_status=any
    Check Response Status Code    404    ${response.status_code}
    ${response}=    GET    url=${url}${cloc}    expected_status=any
    Check Response Status Code    200    ${response.status_code}

    # the clone still serves its frozen copy after purging the original
    &{sheaders}=    Create Dictionary    NGSILD-Snapshot=${clone}[id]
    ${response}=    GET    url=${url}/entities    params=type=Vehicle    headers=${sheaders}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Be Empty    ${response.json()}

    Delete Snapshot    ${cloc}

5161_01_04 Purge Query Restricted To Snapshot Members
    [Documentation]    5.16.7.4: the purge q is restricted to members of the
    ...    Snapshot data type — a q over any other attribute is 400
    ...    BadRequestData and purges nothing. 5.2.41: lastUsedAt is
    ...    initialized at creation time.
    [Tags]    snapshots    5_16_7    5_2_41    since_v1.9.1

    ${loc}=    Create Snapshot    {"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]}
    ${snapshot}=    Wait Until Ready    ${loc}
    Should Not Be Empty    ${snapshot}[lastUsedAt]

    ${response}=    DELETE    url=${url}/snapshots    params=q=speed>50    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Should Contain    ${response.json()}[type]    BadRequestData
    ${response}=    GET    url=${url}${loc}    expected_status=any
    Check Response Status Code    200    ${response.status_code}

    Delete Snapshot    ${loc}

5161_01_05 Snapshot Fill Follows The Distributed Query Behaviour
    [Documentation]    5.16.1.4: snapshot queries are executed "following the
    ...    behaviour described in clause 5.7.2.4" — Entities served by a
    ...    registered Context Source become part of the snapshot alongside
    ...    local ones, and the q applies to all of them.
    [Tags]    snapshots    5_16_1    dist-ops    since_v1.9.1

    &{headers}=    Create Dictionary    Content-Type=application/json
    ${reg_id}=    Set Variable    urn:ngsi-ld:ContextSourceRegistration:snapfed5161
    ${reg}=    Set Variable
    ...    {"id": "${reg_id}", "type": "ContextSourceRegistration", "information": [{"entities": [{"type": "Vehicle"}]}], "operations": ["queryEntity"], "endpoint": "http://${context_source_host}:${context_source_port}"}
    ${response}=    POST    url=${url}/csourceRegistrations    data=${reg}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

    ${response}=    POST    url=${url}/snapshots
    ...    data={"type": "Snapshot", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>50"}]}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${loc}=    Get From Dictionary    ${response.headers}    Location
    ${loc}=    Evaluate    "${loc}".replace("/ngsi-ld/v1", "")

    # the background fill queries the registered Context Source
    Wait For Request    ${15}
    ${path}=    Get Request Url
    Should Contain    ${path}    /ngsi-ld/v1/entities
    Set Reply Header    Content-Type    application/json
    Reply By    200    [{"id": "urn:ngsi-ld:Vehicle:snapfedremote", "type": "Vehicle", "speed": {"type": "Property", "value": 88}}]

    ${snapshot}=    Wait Until Ready    ${loc}
    Should Be Equal    ${snapshot}[snapshotStatus]    success
    &{sheaders}=    Create Dictionary    NGSILD-Snapshot=${snapshot}[id]
    ${response}=    GET    url=${url}/entities    params=type=Vehicle    headers=${sheaders}    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    sorted(e["id"] for e in $response.json())
    Should Contain    ${ids}    urn:ngsi-ld:Vehicle:snapfedremote
    Should Contain    ${ids}    ${fast}
    Should Not Contain    ${ids}    ${slow}

    Delete Snapshot    ${loc}
    ${response}=    DELETE    url=${url}/csourceRegistrations/${reg_id}    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    Stop Context Source Mock Server


*** Keywords ***
Create Snapshot
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${url}/snapshots    data=${payload}    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${loc}=    Get From Dictionary    ${response.headers}    Location
    ${loc}=    Evaluate    "${loc}".replace("/ngsi-ld/v1", "")
    RETURN    ${loc}

Wait Until Ready
    [Arguments]    ${loc}
    FOR    ${i}    IN RANGE    50
        ${response}=    GET    url=${url}${loc}    expected_status=any
        Check Response Status Code    200    ${response.status_code}
        IF    "${response.json()}[snapshotStatus]" != "preparing"    BREAK
        Sleep    0.1s
    END
    RETURN    ${response.json()}

Delete Snapshot
    [Arguments]    ${loc}
    ${response}=    DELETE    url=${url}${loc}    expected_status=any
    Check Response Status Code    204    ${response.status_code}

Create Fixture Entities
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST    url=${url}/entities
    ...    data={"id": "${fast}", "type": "Vehicle", "speed": {"type": "Property", "value": 80}}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${response}=    POST    url=${url}/entities
    ...    data={"id": "${slow}", "type": "Vehicle", "speed": {"type": "Property", "value": 30}}
    ...    headers=${headers}    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Clean Up
    FOR    ${eid}    IN    ${fast}    ${slow}
        ${response}=    DELETE    url=${url}/entities/${eid}    expected_status=any
    END
    ${response}=    DELETE    url=${url}/snapshots    params=q=snapshotPriority>0    expected_status=any
