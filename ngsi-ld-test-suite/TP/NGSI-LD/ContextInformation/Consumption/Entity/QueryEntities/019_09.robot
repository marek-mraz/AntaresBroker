*** Settings ***
Documentation       Check that one can query several entities based on q

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query several entities based on q


*** Variables ***
${entity_type}=     https://ngsi-ld-test-suite/context#Building


*** Test Cases ***    Q    EXPECTED_COUNT
019_09_01 QueryOnName
    [Tags]    e-query    5_7_2
    name=="Eiffel Tower"    1
019_09_02 QueryOnNameAndAlmostFullFalse
    [Tags]    e-query    5_7_2
    name=="Pisa Tower";almostFull==false    1
019_09_03 QueryOnNameAndAlmostFullFalse
    [Tags]    e-query    5_7_2
    name=="Pisa Tower";almostFull==false    1
019_09_04 QueryOnNameOrAlmostFullFalse
    [Tags]    e-query    5_7_2
    name=="Pisa Tower"|almostFull==false    3
019_09_05 QueryOnNamesAndAlmostFullFalse
    [Tags]    e-query    5_7_2
    (name=="Pisa Tower"|name=="Eiffel Tower");almostFull==false    2
019_09_06 QueryOnAirQualityLevel
    [Tags]    e-query    5_7_2
    airQualityLevel==4    1
019_09_07 QueryOnAirQualityLevelRange
    [Tags]    e-query    5_7_2
    airQualityLevel>=4;airQualityLevel<7    2
019_09_08 QueryOnLocatedInExisting
    [Tags]    e-query    5_7_2
    locatedIn==urn:ngsi-ld:City:Pisa    1
019_09_09 QueryOnLocatedInNonExisting
    [Tags]    e-query    5_7_2
    locatedIn==urn:ngsi-ld:City:Paris    0
019_09_10 QueryOnLocatedInTwoCities
    [Tags]    e-query    5_7_2
    locatedIn==urn:ngsi-ld:City:Pisa|locatedIn==urn:ngsi-ld:City:Paris    1


*** Keywords ***
Query several entities based on q
    [Documentation]    Check that one can query several entities based on q query parameter
    [Arguments]    ${q}    ${expected_count}

    ${response}=    Query Entities
    ...    q=${q}
    ...    count=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}
    Check Response Headers Containing NGSILD-Results-Count Equals To    ${expected_count}    ${response.headers}

Setup Initial Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}

    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    building-simple-attributes-second.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

    ${third_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${third_entity_id}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    building-simple-attributes-third.jsonld
    ...    ${third_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${third_entity_id}
