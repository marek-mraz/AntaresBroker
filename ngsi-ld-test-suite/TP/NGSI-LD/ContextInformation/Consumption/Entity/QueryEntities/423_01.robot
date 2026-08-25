*** Settings ***
Documentation       Check the Entity Ordering constructs (CIM 009 clause 4.23) the
...                 official orderBy TPs skip: sort by distance (dist-asc/dist-desc with
...                 the orderFrom reference, 4.23.3 EXAMPLES 8/9), the 4.23.2 mixed-
...                 datatype comparison order (numbers before strings, absent last), and
...                 a trailing [subitem] path (EXAMPLE 4).
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Ordering Entities
Suite Teardown      Delete Ordering Entities


*** Variables ***
${id_prefix}=       urn:ngsi-ld:Building:42301
${id_pattern}=      urn:ngsi-ld:Building:42301.*


*** Test Cases ***
423_01_01 Distance Ascending And Descending
    [Documentation]    4.23.3 EXAMPLES 8/9: dist-asc ranks by ascending haversine
    ...    distance from orderFrom; dist-desc reverses it
    [Tags]    e-query    4_23    since_v1.9.1
    ${ids}=    Query Ordered    location;dist-asc    orderFrom=[8,40]
    Should Be Equal    ${ids}    ${{['${id_prefix}near', '${id_prefix}mid', '${id_prefix}far']}}
    ${ids}=    Query Ordered    location;dist-desc    orderFrom=[8,40]
    Should Be Equal    ${ids}    ${{['${id_prefix}far', '${id_prefix}mid', '${id_prefix}near']}}

423_01_02 Distance Ordering Without OrderFrom Is Rejected
    [Documentation]    4.23.3: "the coordinates element (parameter name orderFrom) shall
    ...    represent the coordinates of the reference geometry" — dist ordering without
    ...    it cannot be satisfied
    [Tags]    e-query    4_23    since_v1.9.1
    ${response}=    Query Ordered Raw    location;dist-asc    ${EMPTY}
    Check Response Status Code    400    ${response.status_code}

423_01_03 Mixed Datatypes Rank Numbers Before Strings Before Absent
    [Documentation]    4.23.2: Numbers < Strings < ... < absent (null values last)
    [Tags]    e-query    4_23    since_v1.9.1
    ${ids}=    Query Ordered    rankProbe    ${EMPTY}
    Should Be Equal    ${ids}    ${{['${id_prefix}near', '${id_prefix}mid', '${id_prefix}far']}}

423_01_04 A Trailing Subitem Path Orders By The Compound Member
    [Documentation]    4.23.3 EXAMPLE 4: orderBy=address[city] addresses a subitem of a
    ...    compound Property value
    [Tags]    e-query    4_23    since_v1.9.1
    ${ids}=    Query Ordered    address[city];desc    ${EMPTY}
    ${first}=    Evaluate    $ids[0]
    Should Be Equal    ${first}    ${id_prefix}far


*** Keywords ***
Query Ordered Raw
    [Arguments]    ${order_by}    ${extra}
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Link=${context_link}
    &{params}=    Create Dictionary    type=Building    idPattern=${id_pattern}    orderBy=${order_by}
    IF    '${extra}' != ''
        ${kv}=    Evaluate    '''${extra}'''.split('=', 1)
        Set To Dictionary    ${params}    ${kv[0]}=${kv[1]}
    END
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    RETURN    ${response}

Query Ordered
    [Arguments]    ${order_by}    ${extra}
    ${response}=    Query Ordered Raw    ${order_by}    ${extra}
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e['id'] for e in $response.json()]
    RETURN    ${ids}

Create Ordering Entities
    # near: location nearest [8,40], rankProbe number, city Amsterdam
    # mid:  location mid,            rankProbe string, city Berlin
    # far:  location farthest,       no rankProbe,     city Zurich
    ${bodies}=    Evaluate
    ...    [ {"id": "${id_prefix}near", "type": "Building", "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [8.01, 40.01]}}, "rankProbe": {"type": "Property", "value": 5}, "address": {"type": "Property", "value": {"city": "Amsterdam"}}}, {"id": "${id_prefix}mid", "type": "Building", "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [9, 41]}}, "rankProbe": {"type": "Property", "value": "abc"}, "address": {"type": "Property", "value": {"city": "Berlin"}}}, {"id": "${id_prefix}far", "type": "Building", "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [10, 45]}}, "address": {"type": "Property", "value": {"city": "Zurich"}}} ]
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    FOR    ${body}    IN    @{bodies}
        ${payload}=    Evaluate    json.dumps($body)    json
        &{headers}=    Create Dictionary    Content-Type=application/json    Link=${context_link}
        ${response}=    POST
        ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
        ...    headers=${headers}
        ...    data=${payload}
        ...    expected_status=any
        Check Response Status Code    201    ${response.status_code}
    END

Delete Ordering Entities
    FOR    ${suffix}    IN    near    mid    far
        Delete Entity    ${id_prefix}${suffix}
    END
