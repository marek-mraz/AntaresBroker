*** Settings ***
Documentation       Check that one can query several entities based on complex geoqueries with Polygon target geometries

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query several entities based on complex geoqueries


*** Test Cases ***    GEOREL    GEOMETRY    COORDINATES    EXPECTED_COUNT
019_11_01 QueryOnEqualsPolygon
    [Documentation]    Check that one can query with an exact polygon
    [Tags]    e-query    5_7_2    4_10
    equals    Polygon    [[[13.2865906,52.5648645],[13.2879639,52.5648645],[13.2797241,52.4988679],[13.477478,52.4712703],[13.5049438,52.5373084],[13.2865906,52.5648645]]]    1
019_11_02 QueryOnIntersectsPolygon
    [Documentation]    Check that one can query with an intersects polygon
    [Tags]    e-query    5_7_2    4_10
    intersects    Polygon    [[[13.3456421,52.4794848],[13.3641815,52.4428773],[13.4177399,52.4730023],[13.3459854,52.479903],[13.3456421,52.4794848]]]    1
019_11_03 QueryOnContainsPolygonMatching
    [Documentation]    Check that one can query with a contains polygon
    [Tags]    e-query    5_7_2    4_10
    contains    Polygon    [[[13.3844376,52.5356966],[13.3844376,52.5356966],[13.381691,52.5123012],[13.4235764,52.5100027],[13.4331894,52.5273425],[13.3844376,52.5356966]]]    1
019_11_04 QueryOnContainsPolygonNotMatching
    [Documentation]    Check that one can query with a contains polygon without matching geometries
    [Tags]    e-query    5_7_2    4_10
    contains    Polygon    [[[13.1369019,52.5198225],[13.1365585,52.5200314],[13.131752,52.4980907],[13.1832504,52.5026887],[13.1369019,52.5198225]]]    0
019_11_05 QueryOnDisjointPolygon
    [Documentation]    Check that one can query with a disjoint polygon
    [Tags]    e-query    5_7_2    4_10
    disjoint    Polygon    [[[13.1369019,52.5198225],[13.1365585,52.5200314],[13.131752,52.4980907],[13.1832504,52.5026887],[13.1369019,52.5198225]]]    2
019_11_06 QueryOnWithinPolygon
    [Documentation]    Check that one can query with a within polygon
    [Tags]    e-query    5_7_2    4_10
    within    Polygon    [[[13.3023834,52.4903066],[13.3027267,52.4905156],[13.2745743,52.465632],[13.3343124,52.4185447],[13.4311295,52.47107],[13.3023834,52.4903066]]]    1
019_11_07 QueryOnOverlapsPolygon
    [Documentation]    Check that one can query with an overlaps polygon
    [Tags]    e-query    5_7_2    4_10
    overlaps    Polygon    [[[13.3456421,52.4794848],[13.3641815,52.4428773],[13.4177399,52.4730023],[13.3459854,52.479903],[13.3456421,52.4794848]]]    1
019_11_08 QueryOnContainsPoint
    [Documentation]    Check that one can query with a contains point
    [Tags]    e-query    5_7_2    4_10
    contains    Point    [13.3442688,52.4650045]    1


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
    ...    building-location-polygon.jsonld
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}

    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    building-location-polygon-second.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
