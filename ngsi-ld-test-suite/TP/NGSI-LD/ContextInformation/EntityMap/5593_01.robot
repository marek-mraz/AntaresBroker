*** Settings ***
Documentation       Verify 5.5.9.3: Pagination with Entity maps.
...
...                 "The set of Entities considered for the result is fixed
...                 with the initial query creating the Entity map … filters
...                 shall be rechecked before returning results … Entities
...                 not or no longer fitting the query shall be removed from
...                 the Entity map during pagination. Pages shall always be
...                 filled to the maximum, as long as Entities are
...                 available."
...
...                 Antares extension TP — no official 5.5.9.3 coverage.

Resource            ${EXECDIR}/resources/ApiUtils/entityMap.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Library             Collections

Suite Setup         Create Five Entities And An EntityMap
Suite Teardown      Delete Fixture Entities And EntityMap


*** Variables ***
${id_prefix}=       urn:ngsi-ld:Vehicle:em5593


*** Test Cases ***
5593_01_01 Pages Fill To The Maximum And Links Walk The Original Query
    [Documentation]    5.5.9.3/6.3.10: limit=2 over a 5-entity map — the
    ...    first page holds 2 Entities and a next link with offset=2; the
    ...    last page holds the remainder, a prev link, and no next link.
    ...    The links carry the original query, never the page's id list.
    [Tags]    em-paging    5_5_9_3    since_v1.9.1
    &{params}=    Create Dictionary    type=Vehicle    limit=2
    &{headers}=    Create Dictionary    NGSILD-EntityMap=${entityMapLocation}
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    ${link}=    Get From Dictionary    ${response.headers}    Link
    Should Contain    ${link}    rel="next"
    Should Contain    ${link}    offset=2
    Should Not Contain    ${link}    ${id_prefix}

    &{params}=    Create Dictionary    type=Vehicle    limit=2    offset=4
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    1
    ${link}=    Get From Dictionary    ${response.headers}    Link
    Should Not Contain    ${link}    rel="next"
    Should Contain    ${link}    rel="prev"
    Should Contain    ${link}    offset=2

5593_01_02 An Entity Deleted After Map Creation Is Pruned During Pagination
    [Documentation]    5.5.9.3: the deleted Entity no longer fits the query —
    ...    the page skips it, still fills to the maximum from the remaining
    ...    candidates, and the Entity is removed from the EntityMap itself.
    [Tags]    em-paging    5_5_9_3    since_v1.9.1
    ${response}=    Delete EntityMap Test Entity    ${id_prefix}-1
    Check Response Status Code    204    ${response.status_code}

    &{params}=    Create Dictionary    type=Vehicle    limit=3
    &{headers}=    Create Dictionary    NGSILD-EntityMap=${entityMapLocation}
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    3
    Should Not Contain    ${response.text}    ${id_prefix}-1

    ${response}=    Retrieve EntityMap    ${entityMapId}
    Check Response Status Code    200    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.json()['entityMap']}    ${id_prefix}-1
    Dictionary Should Contain Key    ${response.json()['entityMap']}    ${id_prefix}-0

5593_01_03 Count Walks The Whole Map
    [Documentation]    6.3.10 count with a map-based query: the NGSILD-
    ...    Results-Count is the matching total over every candidate in the
    ...    map (4 after the deletion above), not the page size.
    [Tags]    em-paging    5_5_9_3    since_v1.9.1
    &{params}=    Create Dictionary    type=Vehicle    limit=2    count=true
    &{headers}=    Create Dictionary    NGSILD-EntityMap=${entityMapLocation}
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Length Should Be    ${response.json()}    2
    ${count}=    Get From Dictionary    ${response.headers}    NGSILD-Results-Count
    Should Be Equal As Integers    ${count}    4


*** Keywords ***
Create Five Entities And An EntityMap
    FOR    ${i}    IN RANGE    5
        ${response}=    Create EntityMap Test Entity    ${id_prefix}-${i}    ${50}
        Check Response Status Code    201    ${response.status_code}
    END
    &{params}=    Create Dictionary    type=Vehicle    entityMap=true
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    ${location}=    Get From Dictionary    ${response.headers}    NGSILD-EntityMap
    Set Suite Variable    ${entityMapLocation}    ${location}
    ${entityMapId}=    Fetch EntityMap Id From Response    ${response.headers}
    Set Suite Variable    ${entityMapId}

Delete Fixture Entities And EntityMap
    FOR    ${i}    IN RANGE    5
        Delete EntityMap Test Entity    ${id_prefix}-${i}
    END
    Delete EntityMap    ${entityMapId}
