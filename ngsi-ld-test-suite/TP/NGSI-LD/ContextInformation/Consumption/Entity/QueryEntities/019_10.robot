*** Settings ***
Documentation       Check that one can query several entities based on complex geoqueries with Point target geometries

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query several entities based on complex geoqueries


*** Test Cases ***    GEOREL    GEOMETRY    COORDINATES    EXPECTED_COUNT
019_10_01 QueryOnEqualsPoint
    [Documentation]    Check that one can query with an exact point
    [Tags]    e-query    5_7_2    4_10
    equals    Point    [13.3986, 52.5547]    1
019_10_02 QueryOnMaxDistancePoint
    [Documentation]    Check that one can query with a max distance
    [Tags]    e-query    5_7_2    4_10
    near;maxDistance==15000    Point    [13.388930, 52.534473]    2
019_10_03 QueryOnMinDistancePoint
    [Documentation]    Check that one can query with a min distance
    [Tags]    e-query    5_7_2    4_10
    near;minDistance==15000    Point    [13.388930, 52.534473]    1
019_10_04 QueryOnWithinPolygon
    [Documentation]    Check that one can query within a polygon
    [Tags]    e-query    5_7_2    4_10
    within    Polygon    [[[13.1616211,52.5438124],[13.597641,52.4840224],[13.5948944,52.3402782],[13.142395,52.386428],[13.1616211,52.5438124]]]    2
019_10_05 QueryOnDisjointPolygon
    [Documentation]    Check that one can query with a disjoint polygon
    [Tags]    e-query    5_7_2    4_10
    disjoint    Polygon    [[[13.1616211,52.5438124],[13.597641,52.4840224],[13.5948944,52.3402782],[13.142395,52.386428],[13.1616211,52.5438124]]]    1
019_10_06 QueryOnDisjointPoint
    [Documentation]    Check that one can query with a disjoint point
    [Tags]    e-query    5_7_2    4_10
    disjoint    Point    [13.3986, 52.5547]    2
019_10_07 QueryOnContainsPoint
    [Documentation]    Check that one can query with a contains point
    [Tags]    e-query    5_7_2    4_10
    contains    Point    [13.3986, 52.5547]    1
019_10_08 QueryOnIntersectsPoint
    [Documentation]    Check that one can query with an intersects point
    [Tags]    e-query    5_7_2    4_10
    intersects    Point    [13.3986, 52.5547]    1


*** Keywords ***
Query several entities based on complex geoqueries
    [Documentation]    Query with a complex geoquery and check the number of results
    [Arguments]    ${georel}    ${geometry}    ${coordinates}    ${expected_count}

    ${response}=    Query Entities
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    ...    georel=${georel}
    ...    geometry=${geometry}
    ...    coordinates=${coordinates}
    ...    count=true

    Check Response Status Code    200    ${response.status_code}
    Check Response Headers Containing NGSILD-Results-Count Equals To    ${expected_count}    ${response.headers}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    building-location-attribute.jsonld
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}

    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    building-location-attribute-second.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

    ${third_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${third_entity_id}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    building-location-attribute-third.jsonld
    ...    ${third_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${third_entity_id}
