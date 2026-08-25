*** Settings ***
Documentation       Verify 5.7.3.4 / 5.7.4.4 EntityMap usage on the Temporal
...                 Evolution retrieve and query.
...
...                 5.7.3.4 / 5.7.4.4: "If a flag to return an EntityMap was
...                 present in the request, and no EntityMap currently
...                 exists, then a new EntityMap shall be created"; a live
...                 referenced map fixes the query's result set (5.5.14) and
...                 its location is echoed; an unknown or expired reference
...                 creates a new map.
...                 Antares extension TP — no official EntityMap coverage.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Test Setup          Create Fixture Entities
Test Teardown       Delete Fixture Entities


*** Variables ***
${vehicle_one}=     urn:ngsi-ld:Vehicle:em5734one
${vehicle_two}=     urn:ngsi-ld:Vehicle:em5734two
${window}=          timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt


*** Test Cases ***
5734_01_01 Temporal Retrieve With EntityMap Flag Creates A Map
    [Documentation]    5.7.3.4: entityMap=true on GET /temporal/entities/{id}
    ...    → a new map holding the entity under "@none" (5.2.39).
    [Tags]    em-usage    5_7_3_4    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/temporal/entities/${vehicle_one}
    ...    params=${window}&entityMap=true
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Contain Key    ${response.headers}    NGSILD-EntityMap
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    ${map}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    200    ${map.status_code}
    ${sources}=    Get From Dictionary    ${map.json()['entityMap']}    ${vehicle_one}
    Should Be Equal As Strings    ${sources}    ['@none']
    Dictionary Should Not Contain Key    ${map.json()['entityMap']}    ${vehicle_two}
    Delete EntityMap    ${entityMapId}

5734_01_02 Live EntityMap Fixes The Temporal Query Result Set
    [Documentation]    5.7.4.4 / 5.5.14: the map (narrowed to one id at
    ...    creation) fixes a broader follow-up query — the other Entity
    ...    must NOT be returned; the map location is echoed.
    [Tags]    em-usage    5_7_4_4    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/temporal/entities
    ...    params=type=Vehicle&id=${vehicle_one}&${window}&entityMap=true
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${maploc}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap

    &{headers}=    Create Dictionary    NGSILD-EntityMap=${maploc}
    ${response}=    GET
    ...    url=${url}/temporal/entities
    ...    params=type=Vehicle&${window}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    sorted(e["id"] for e in $response.json())
    Should Be Equal As Strings    ${ids}    ['${vehicle_one}']
    Should Not Contain    ${ids}    ${vehicle_two}
    ${echoed}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap
    Should Be Equal    ${echoed}    ${maploc}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}

5734_01_03 Unknown EntityMap Reference Recreates On The Temporal Query
    [Documentation]    5.7.4.4: an unknown/expired NGSILD-EntityMap reference
    ...    → "a new EntityMap shall be created" (201 + fresh location, the
    ...    dead reference must NOT be echoed).
    [Tags]    em-usage    5_7_4_4    since_v1.9.1

    &{headers}=    Create Dictionary    NGSILD-EntityMap=urn:ngsi-ld:entitymap:em5734dead
    ${response}=    GET
    ...    url=${url}/temporal/entities
    ...    params=type=Vehicle&${window}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${loc}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap
    Should Not Contain    ${loc}    em5734dead
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Delete EntityMap    ${entityMapId}


*** Keywords ***
Create Fixture Entities
    ${response}=    Create EntityMap Test Entity    ${vehicle_one}    30
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Create EntityMap Test Entity    ${vehicle_two}    90
    Check Response Status Code    201    ${response.status_code}

Delete Fixture Entities
    Delete EntityMap Test Entity    ${vehicle_one}
    Delete EntityMap Test Entity    ${vehicle_two}
