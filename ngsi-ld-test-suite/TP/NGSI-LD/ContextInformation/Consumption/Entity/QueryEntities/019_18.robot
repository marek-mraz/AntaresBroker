*** Settings ***
Documentation       Check that a BadRequestDataException is returned if the Query Entities request is invalid with respect to pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities
Test Template       Query Entities With Invalid Pick Or Omit Query Params Usage


*** Test Cases ***    PICK    OMIT    ATTRS
019_18_01 RetrieveWithSameEntityMemberInPickAndOmit
    [Documentation]    Check that a BadRequestDataException is returned if an entity member is present in pick and omit
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    name    name    ${EMPTY}
019_18_02 RetrieveWithPickAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if pick and attrs query params are present
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    name    ${EMPTY}    category
019_18_03 RetrieveWithOmitAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if omit and attrs query params are present
    [Tags]    e-query    5_7_2    4_21    since_v1.8.1
    ${EMPTY}    name    category
019_18_04 RetrieveWithInvalidCharacter
    [Documentation]    Check that a BadRequestDataException is returned if an invalid character is present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id;name    ${EMPTY}    ${EMPTY}
019_18_05 RetrieveWithUnclosedBrace
    [Documentation]    Check that a BadRequestDataException is returned if a brace is not closed
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{name    ${EMPTY}    ${EMPTY}
019_18_06 RetrieveWithDoubleBraces
    [Documentation]    Check that a BadRequestDataException is returned if a double brace is present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{{name}    ${EMPTY}    ${EMPTY}
019_18_07 RetrieveWithConsecutiveSeparators
    [Documentation]    Check that a BadRequestDataException is returned if consecutive separators are present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,,name    ${EMPTY}    ${EMPTY}
019_18_08 RetrieveWithExpressionStartingWithSpecialCharacter
    [Documentation]    Check that a BadRequestDataException is returned if expression starts with a special character
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{,name}    ${EMPTY}    ${EMPTY}
019_18_09 RetrieveWithExpressionContainingNoAttribute
    [Documentation]    Check that a BadRequestDataException is returned if expression does not contain an attribute
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{}    ${EMPTY}    ${EMPTY}


*** Keywords ***
Query Entities With Invalid Pick Or Omit Query Params Usage
    [Documentation]    Check that a BadRequestDataException is returned if the Query Entities request is invalid with respect to pick and omit query params
    [Arguments]    ${pick}    ${omit}    ${attrs}

    ${response}=    Query Entities
    ...    entity_types=Building,Vehicle
    ...    pick=${pick}
    ...    omit=${omit}
    ...    attrs=${attrs}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Setup Initial Entities
    ${first_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}019-18-1
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}

    ${second_entity_id}=    Catenate    ${VEHICLE_ID_PREFIX}019-18-2
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    vehicle-simple-attributes.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

    ${third_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}019-18-3
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
