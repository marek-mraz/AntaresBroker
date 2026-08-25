*** Settings ***
Documentation       Verify 6.3.10/6.3.13 pagination and count on a temporal
...                 query WITH a values filter: pages partition the
...                 q-matching set exactly (no overlap, no omission), the
...                 count reflects only true matches, and an entity whose
...                 only q-matching instance lies outside the window never
...                 appears on any page or in the count. On pg/timescale CI
...                 cells these cases exercise the store-side exact-q
...                 entity-paging pushdown. Antares extension TP.

Library             RequestsLibrary
Library             Collections
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Create Fixture Entities
Suite Teardown      Delete Fixture Entities


*** Variables ***
${p1}=          urn:ngsi-ld:Vehicle:qpage5744-one
${p2}=          urn:ngsi-ld:Vehicle:qpage5744-two
${decoy}=       urn:ngsi-ld:Vehicle:qpage5744-decoy
${window}=      timerel=between&timeAt=2026-03-01T00:00:00Z&endTimeAt=2026-03-02T00:00:00Z


*** Test Cases ***
5744_05_01 Count Reflects Only True Matches
    [Documentation]    6.3.13: count=true with q=speed>25 — the decoy whose
    ...    matching instance is out-of-window is not counted.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&q=speed%3E25&count=true&limit=1&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal As Strings    ${response.headers['NGSILD-Results-Count']}    2

5744_05_02 Pages Partition The Matching Set
    [Documentation]    6.3.10: limit=1 pages through the q-matching set —
    ...    both matches appear exactly once across the two pages, the
    ...    decoy on neither.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${page1}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&q=speed%3E25&limit=1&offset=0&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${page1.status_code}
    ${page2}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&q=speed%3E25&limit=1&offset=1&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${page2.status_code}
    ${both}=    Catenate    ${page1.text}    ${page2.text}
    Should Contain    ${both}    ${p1}
    Should Contain    ${both}    ${p2}
    Should Not Contain    ${both}    ${decoy}
    Should Not Contain    ${page1.text}    ${p2}
    Should Not Contain    ${page2.text}    ${p1}

5744_05_03 Page Past The End Is Empty
    [Documentation]    6.3.10: an offset beyond the q-matching set returns
    ...    an empty array, never the decoy.
    [Tags]    te-query    5_7_4    since_v1.9.1
    ${response}=    GET
    ...    url=${temporal_api_url}/temporal/entities
    ...    params=type=Vehicle&q=speed%3E25&limit=1&offset=2&${window}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    qpage5744


*** Keywords ***
Create Temporal Entity
    [Arguments]    ${payload}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    POST
    ...    url=${temporal_api_url}/temporal/entities
    ...    data=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}

Create Fixture Entities
    Create Temporal Entity    {"id": "${p1}", "type": "Vehicle", "speed": [{"type": "Property", "value": 30, "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${p2}", "type": "Vehicle", "speed": [{"type": "Property", "value": 40, "observedAt": "2026-03-01T12:00:00Z"}]}
    Create Temporal Entity    {"id": "${decoy}", "type": "Vehicle", "speed": [{"type": "Property", "value": 90, "observedAt": "2026-02-28T23:00:00Z"}, {"type": "Property", "value": 5, "observedAt": "2026-03-01T12:00:00Z"}]}

Delete Fixture Entities
    FOR    ${id}    IN    ${p1}    ${p2}    ${decoy}
        ${response}=    DELETE    url=${temporal_api_url}/temporal/entities/${id}    expected_status=any
    END
