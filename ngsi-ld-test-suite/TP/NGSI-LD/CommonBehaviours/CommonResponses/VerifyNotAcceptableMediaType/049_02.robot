*** Settings ***
Documentation       Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${accept}=          application/geo+json
${status_code}=     406


*** Test Cases ***
049_02_01 Retrieve Subscription By Id
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get /subscriptions/{subscriptionId})
    [Tags]    sub-retrieve    cb-noacceptable-medtype    6_3_4
    ${id}=    Generate Random Subscription Id
    ${response}=    Retrieve Subscription
    ...    id=${id}
    ...    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}

049_02_02 Query Temporal Entities
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get /temporal/entities)
    [Tags]    te-query    cb-noacceptable-medtype    6_3_4
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    Vehicle
    ${response}=    Query Temporal Representation Of Entities
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    timerel=after
    ...    timeAt=2020-08-01T12:05:00Z
    ...    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}

049_02_03 Query Context Source Registration
    [Documentation]    Verify throwing 406 HTTP status code (Not Acceptable Media Type) if the "Accept" header is "application/geo+json" for operations different than "Retrieve Entity" and "Query Entity" (get /csourceRegistrations)
    [Tags]    csr-query    cb-noacceptable-medtype    6_3_4
    ${response}=    Query Context Source Registrations With Return    type=Building    accept=${accept}
    Check Response Status Code    ${status_code}    ${response.status_code}
